// Headless check of the page an engine serves, with screenshots.
//   node scripts/check-page.mjs http://127.0.0.1:7787 <payout UA> ../../../docs/screenshots
//
// Checks: the page loads, the live board renders, round 0 verifies 5/5 in
// the browser (started from the proof strip), nothing offers the bare vault
// address to pay (no QR or copy button before a request exists), "Create
// payment" builds a zcash: URI and a QR, editing the tickets or the payout
// address withdraws that request, rounds whose entries were all refunded say
// "refunded", no console errors, no horizontal overflow at 390 px, text
// contrast, keyboard reachability with a visible focus ring.
// Screenshots are taken with prefers-reduced-motion, so nothing moves.
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const [url = "http://127.0.0.1:7787", payout, shots = "../../../docs/screenshots"] = process.argv.slice(2);
if (!payout) throw new Error("usage: check-page.mjs <url> <payout UA> [screenshot dir]");
mkdirSync(shots, { recursive: true });
const result = {};
const errors = [];

function watch(page, tag) {
  page.on("pageerror", (e) => errors.push(`${tag} pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`${tag} console: ${m.text()}`);
  });
}

// Text contrast against the nearest opaque background (WCAG 2 ratio).
function contrastAudit() {
  const parse = (c) => {
    const m = c.match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    const [r, g, b, a = 1] = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
    return { r, g, b, a };
  };
  const lum = ({ r, g, b }) => {
    const f = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const bgOf = (el) => {
    for (let e = el; e; e = e.parentElement) {
      const c = parse(getComputedStyle(e).backgroundColor);
      if (c && c.a >= 0.99) return c;
    }
    return { r: 9, g: 9, b: 10 };
  };
  const fails = [];
  let checked = 0;
  let min = 21;
  for (const el of document.querySelectorAll(".zc *")) {
    const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (!own || el.closest("svg, [aria-hidden='true'], .sr-only, button:disabled")) continue;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none" || Number(st.opacity) < 0.99) continue;
    const fg = parse(st.color);
    if (!fg) continue;
    const bg = bgOf(el);
    const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
    const ratio = (a + 0.05) / (b + 0.05);
    const size = parseFloat(st.fontSize);
    const large = size >= 24 || (size >= 18.66 && Number(st.fontWeight) >= 700);
    checked++;
    min = Math.min(min, ratio);
    if (ratio < (large ? 3 : 4.5)) fails.push({ text: el.textContent.trim().slice(0, 40), ratio: Number(ratio.toFixed(2)), size });
  }
  return { checked, min: Number(min.toFixed(2)), fails };
}

const browser = await chromium.launch();
try {
  // 1. Desktop, reduced motion: the functional checks and the screenshot.
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const page = await ctx.newPage();
  watch(page, "desktop");
  const t0 = Date.now();
  await page.goto(url, { waitUntil: "load" });
  result.loadMs = Date.now() - t0;
  result.title = await page.title();
  await page.locator(".round-title").waitFor({ timeout: 20000 });
  await page.locator(".past-rounds .rounds-list li").first().waitFor({ timeout: 20000 });
  result.boardMs = Date.now() - t0;
  result.board = {
    round: (await page.locator(".round-title").textContent())?.trim(),
    status: (await page.locator(".current-round .chip").textContent())?.trim(),
    countdown: (await page.locator(".countdown").innerText()).replace(/\s+/g, " ").trim(),
    stats: (await page.locator(".stat-grid").innerText()).replace(/\s+/g, " ").trim(),
  };
  result.pastRounds = (await page.locator(".past-rounds .rounds-list > li").allInnerTexts()).map((t) => t.replace(/\s+/g, " ").trim());
  // A round whose entries were all refunded (no tickets sold, refunds paid)
  // is labelled "refunded", never "carried over" (nothing was carried).
  result.refundedRounds = await page.evaluate(async () => {
    const { rounds } = await (await fetch("/v1/rounds?limit=50")).json();
    return rounds
      .filter((r) => r.status === "no_win" && r.refunds > 0 && r.game.tickets_sold === 0)
      .map((r) => {
        const row = document.querySelector(`.round-row[data-round="${r.round}"]`);
        return row ? { round: r.round, label: row.querySelectorAll("dd")[1]?.textContent?.trim() } : null;
      })
      .filter(Boolean);
  });
  // Before a payment request exists, nothing on the page offers the bare
  // vault address to pay: no QR, no copy button in step 1, and step 1 points
  // to the step-3 link (a payment without its memo can't be refunded).
  result.step1 = await page.evaluate(() => {
    const step = document.querySelector(".enter-steps > li");
    return {
      qrs: document.querySelectorAll(".how-to-enter .qr").length,
      buttons: step ? step.querySelectorAll("button").length : -1,
      text: step ? step.innerText.replace(/\s+/g, " ").trim() : "",
    };
  });
  result.terminal = await page.locator(".term-body").innerText();

  // Verify round 0 from the proof strip.
  const tv = Date.now();
  await page.getByRole("button", { name: "Verify round 0 yourself" }).click();
  const panel = page.locator("#verify");
  await page.waitForFunction(() => ["passed", "failed", "error"].includes(document.querySelector("#verify")?.dataset.phase ?? ""), null, { timeout: 300000 });
  result.verifyWallMs = Date.now() - tv;
  result.verifyPhase = await panel.getAttribute("data-phase");
  result.verifyRound = await panel.getAttribute("data-round");
  result.verifyVerdict = (await panel.locator(".verify-verdict").textContent())?.trim();
  result.checks = (await panel.locator(".check").allInnerTexts()).map((t) => t.replace(/\s+/g, " ").trim());
  result.verifyInView = await panel.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return r.top < window.innerHeight && r.bottom > 0;
  });

  result.contrast = await page.evaluate(contrastAudit);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: `${shots}/desktop.png`, fullPage: true });

  // Keyboard: every control is reachable with Tab and shows a focus ring.
  const controls = await page.evaluate(() => {
    const sel = "a[href], button:not([disabled]), input:not([disabled]), summary, [tabindex='0']";
    const all = [...document.querySelectorAll(sel)].filter((el) => el.getClientRects().length > 0);
    all.forEach((el, i) => el.setAttribute("data-kb", String(i)));
    return all.length;
  });
  const reached = new Set();
  const noRing = [];
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  for (let i = 0; i < controls + 10; i++) {
    await page.keyboard.press("Tab");
    const f = await page.evaluate(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return null;
      const st = getComputedStyle(el);
      return { id: el.getAttribute("data-kb"), ring: st.outlineStyle !== "none" && parseFloat(st.outlineWidth) >= 2, label: (el.getAttribute("aria-label") || el.textContent || el.id || el.tagName).trim().slice(0, 40) };
    });
    if (!f || f.id === null) continue;
    reached.add(f.id);
    if (!f.ring) noRing.push(f.label);
  }
  result.keyboard = { controls, reached: reached.size, noFocusRing: [...new Set(noRing)] };

  // Create payment (wait for an open round with enough blocks left).
  await page.getByLabel("Tickets").fill("2");
  await page.getByLabel(/Your payout address/).fill(payout);
  const create = page.getByRole("button", { name: "Create payment" });
  const link = page.locator(".entry-request a", { hasText: "Open payment link" });
  const createPayment = async () => {
    await create.waitFor({ state: "visible" });
    await page.waitForFunction(() => {
      const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === "Create payment");
      return b && !b.disabled;
    }, null, { timeout: 600000 });
    await create.click();
    await link.waitFor({ timeout: 60000 });
  };
  await createPayment();
  result.entryUri = await link.getAttribute("href");
  result.entryText = (await page.locator(".entry-text > span").first().textContent())?.trim();
  result.entryQr = await page.locator(".entry-request img.qr").evaluate((img) => img.src.startsWith("data:image/png") && img.naturalWidth > 0);
  // A built request is withdrawn as soon as what it was built from changes,
  // so step 3 never offers a link for other tickets or another address.
  await page.getByLabel("Tickets").fill("3");
  result.requestAfterTicketEdit = await page.locator(".entry-request").count();
  await createPayment();
  result.rebuiltText = (await page.locator(".entry-text > span").first().textContent())?.trim();
  await page.getByLabel(/Your payout address/).fill("");
  result.requestAfterPayoutEdit = await page.locator(".entry-request").count();
  // An edit made while a request is still being sealed (the first one loads
  // the wasm) wins: the late result is dropped, not shown for the old inputs.
  const fresh = await ctx.newPage();
  watch(fresh, "race");
  await fresh.goto(url, { waitUntil: "load" });
  await fresh.getByLabel("Tickets").fill("2");
  await fresh.getByLabel(/Your payout address/).fill(payout);
  await fresh.waitForFunction(() => {
    const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === "Create payment");
    return b && !b.disabled;
  }, null, { timeout: 600000 });
  await fresh.getByRole("button", { name: "Create payment" }).click();
  await fresh.getByLabel("Tickets").fill("3");
  await fresh.waitForTimeout(3000);
  result.requestAfterEditWhileSealing = await fresh.locator(".entry-request").count();
  await ctx.close();

  // 2. Desktop with motion: animations run without errors.
  const moving = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  watch(moving, "motion");
  await moving.goto(url, { waitUntil: "load" });
  await moving.locator(".term-body").scrollIntoViewIfNeeded();
  await moving.waitForTimeout(9000);
  result.terminalAfterAnimation = (await moving.locator(".term-body").innerText()).trim().split("\n").slice(-1)[0];
  result.heroAnimating = await moving.evaluate(() => getComputedStyle(document.querySelector(".ha-coin")).animationName);
  await moving.close();

  // 3. Mobile, 390 px, reduced motion.
  const mctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const mobile = await mctx.newPage();
  watch(mobile, "mobile");
  await mobile.goto(url, { waitUntil: "load" });
  await mobile.locator(".round-title").waitFor({ timeout: 20000 });
  await mobile.locator(".past-rounds .rounds-list li").first().waitFor({ timeout: 20000 });
  result.mobile = await mobile.evaluate(() => {
    const w = window.innerWidth;
    const scrolls = (el) => {
      for (let e = el.parentElement; e && !e.classList.contains("zc"); e = e.parentElement) {
        const o = getComputedStyle(e).overflowX;
        if (o === "auto" || o === "scroll" || o === "hidden") return true;
      }
      return false;
    };
    const over = [...document.querySelectorAll(".zc *")]
      .filter((el) => el.getBoundingClientRect().right > w + 1 && !scrolls(el))
      .map((el) => el.className || el.tagName)
      .slice(0, 10);
    return { horizontalScroll: document.documentElement.scrollWidth > w, overflowing: over };
  });
  await mobile.screenshot({ path: `${shots}/mobile-390.png`, fullPage: true });
  await mctx.close();
  result.errors = errors;
} finally {
  await browser.close();
}
console.log(JSON.stringify(result, null, 2));
const ok =
  result.verifyPhase === "passed" &&
  result.checks?.length === 5 &&
  result.checks.every((c) => / pass$/.test(c)) &&
  /^zcash:/.test(result.entryUri ?? "") &&
  result.entryQr === true &&
  result.step1.qrs === 0 &&
  result.step1.buttons === 0 &&
  /payment link in step 3/.test(result.step1.text) &&
  result.requestAfterTicketEdit === 0 &&
  /for 3 tickets/.test(result.rebuiltText ?? "") &&
  result.requestAfterPayoutEdit === 0 &&
  result.requestAfterEditWhileSealing === 0 &&
  result.refundedRounds.every((r) => r.label === "refunded") &&
  errors.length === 0 &&
  !result.mobile.horizontalScroll &&
  result.mobile.overflowing.length === 0 &&
  result.contrast.fails.length === 0 &&
  result.keyboard.reached === result.keyboard.controls &&
  result.keyboard.noFocusRing.length === 0;
process.exit(ok ? 0 : 1);
