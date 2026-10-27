// Runs the five round checks in WebAssembly, off the main thread. Every
// block and transaction comes through the relay (/relay); the proof bundle
// and the public config come from the engine.
import init, { verifyPlan, verifyRound, verifyScan } from "./wasm/zecraft_raffle_wasm.js";

export interface Check {
  name: string;
  pass: boolean;
  status: "pass" | "warn" | "fail" | "not_available";
  summary: string;
  detail: string[];
}
export interface Report {
  round: number;
  passed: boolean;
  checks: Check[];
}
export type WorkerMessage =
  | { kind: "progress"; step: string }
  | { kind: "done"; report: Report; millis: number }
  | { kind: "error"; message: string };

const post = (m: WorkerMessage) => self.postMessage(m);

async function bytes(url: string): Promise<Uint8Array> {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

self.onmessage = async (event: MessageEvent<{ round: number }>) => {
  const started = performance.now();
  try {
    post({ kind: "progress", step: "Loading the verifier" });
    await init();
    const round = event.data.round;
    const [bundle, config] = await Promise.all([
      fetch(`/v1/rounds/${round}/proof`).then((r) => r.text()),
      fetch("/v1/config").then((r) => r.text()),
    ]);
    const plan = JSON.parse(verifyPlan(bundle)) as { start: number; end: number; payout_height: number | null };
    post({ kind: "progress", step: `Fetching blocks ${plan.start} to ${plan.end} through the relay` });
    const parts = [await bytes(`/relay/v1/chain/blocks?start=${plan.start}&end=${plan.end}`)];
    const p = plan.payout_height;
    if (p !== null && (p < plan.start || p > plan.end)) {
      parts.push(await bytes(`/relay/v1/chain/blocks?start=${p}&end=${p}`));
    }
    const blocks = concat(parts);
    post({ kind: "progress", step: "Finding the vault's payments with the public viewing key" });
    const txids = JSON.parse(verifyScan(bundle, config, blocks)) as string[];
    post({ kind: "progress", step: `Fetching ${txids.length} transactions` });
    const txs = [];
    for (const txid of txids) txs.push(await bytes(`/relay/v1/tx/${txid}`));
    post({ kind: "progress", step: "Running the five checks (the zero-knowledge proof check takes the longest)" });
    const report = JSON.parse(verifyRound(bundle, config, blocks, txs)) as Report;
    post({ kind: "done", report, millis: Math.round(performance.now() - started) });
  } catch (e) {
    post({ kind: "error", message: e instanceof Error ? e.message : String(e) });
  }
};
