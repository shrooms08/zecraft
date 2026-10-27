// Headless check of the page an engine serves, with screenshots.
//   node scripts/check-page.mjs http://127.0.0.1:7787 <payout UA> ../../../docs/screenshots
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const [url = "http://127.0.0.1:7787", payout, shots = "../../../docs/screenshots"] = process.argv.slice(2);
mkdirSync(shots, { recursive: true });
const result = {};
const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const t0 = Date.now();
  await page.goto(url, { waitUntil: "load" });
  result.loadMs = Date.now() - t0;
  const nav = await page.evaluate(() => {
    const n = performance.getEntriesByType("navigation")[0];
    return { domContentLoaded: Math.round(n.domContentLoadedEventEnd), load: Math.round(n.loadEventEnd) };
  });
  result.navigation = nav;
  result.title = await page.title();
  await page.getByText(/^Round \d+/).first().waitFor({ timeout: 20000 });
  result.boardMs = Date.now() - t0;
  result.currentRound = (await page.locator(".current-round h3").textContent())?.trim();
  const row0 = page.locator(".past-rounds tbody tr").filter({ has: page.locator('th:text-is("0")') });
  result.round0Status = (await row0.locator("td").first().textContent())?.trim();
  result.round0Row = (await row0.textContent())?.trim();

  // Entry form: the request is sealed in the browser.
  await page.getByLabel("Tickets").fill("2");
  await page.getByLabel("Your fresh payout address").fill(payout);
  await page.getByRole("button", { name: /Build the request/ }).click();
  const uri = page.locator(".entry-request .mono").first();
  await uri.waitFor({ timeout: 60000 });
  result.entryUri = (await uri.textContent())?.trim();

  // Verify round 0 in the browser.
  const tv = Date.now();
  await page.locator('.verify[data-round="0"] button').click();
  const verdict = page.locator('.verify[data-round="0"] .verify-result');
  await verdict.waitFor({ timeout: 300000 });
  result.verifyMs = Date.now() - tv;
  result.verifyHeadline = (await verdict.locator("p").first().textContent())?.trim();
  result.checks = await verdict.locator(".checks li").allTextContents();
  await page.screenshot({ path: `${shots}/desktop.png`, fullPage: true });

  const mobile = await browser.newPage({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true });
  await mobile.goto(url, { waitUntil: "load" });
  await mobile.getByText(/^Round \d+/).first().waitFor({ timeout: 20000 });
  await mobile.screenshot({ path: `${shots}/mobile-390.png`, fullPage: true });
  result.mobileHorizontalScroll = await mobile.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  result.pageErrors = errors;
} finally {
  await browser.close();
}
console.log(JSON.stringify(result, null, 2));
const ok = result.round0Status === "paid" && /^zcash:/.test(result.entryUri ?? "") && result.checks?.length === 5 && result.checks.every((c) => c.startsWith("PASS"));
process.exit(ok ? 0 : 1);
