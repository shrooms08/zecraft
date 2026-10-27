// Runs the wasm verifier in Node against a saved proof bundle, fetching the
// game config from the engine and every block and transaction through a
// Zecraft relay, and compares the five results with the CLI's saved output.
//
//   node games/raffle/wasm/tests/verify-node.mjs <pkg dir (wasm-bindgen --target nodejs)> \
//        docs/examples/raffle-round-0.proof.json docs/examples/raffle-round-0.verify.txt \
//        http://127.0.0.1:7787 http://127.0.0.1:7790
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import path from "node:path";

const [pkg, bundlePath, cliPath, engine, relay] = process.argv.slice(2);
const wasm = createRequire(import.meta.url)(path.resolve(pkg, "zecraft_raffle_wasm.js"));

async function bytes(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status} ${await r.text()}`);
  return new Uint8Array(await r.arrayBuffer());
}
function concat(parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
}

const started = Date.now();
const bundle = readFileSync(bundlePath, "utf8");
const config = await (await fetch(`${engine}/v1/config`)).text();
const plan = JSON.parse(wasm.verifyPlan(bundle));
const parts = [await bytes(`${relay}/v1/chain/blocks?start=${plan.start}&end=${plan.end}`)];
if (plan.payout_height && (plan.payout_height < plan.start || plan.payout_height > plan.end)) {
  parts.push(await bytes(`${relay}/v1/chain/blocks?start=${plan.payout_height}&end=${plan.payout_height}`));
}
const blocks = concat(parts);
const txids = JSON.parse(wasm.verifyScan(bundle, config, blocks));
const txs = [];
for (const txid of txids) txs.push(await bytes(`${relay}/v1/tx/${txid}`));
const fetched = Date.now();
const report = JSON.parse(wasm.verifyRound(bundle, config, blocks, txs));
const done = Date.now();

for (const [i, c] of report.checks.entries()) {
  console.log(`${i + 1}. [${c.pass ? "PASS" : "FAIL"}] ${c.name}: ${c.summary}`);
}
console.log(`fetch ${fetched - started} ms (${blocks.length} bytes of blocks, ${txs.length} txs), checks ${done - fetched} ms`);

const cli = [...readFileSync(cliPath, "utf8").matchAll(/^\d\. \[(PASS|FAIL|WARN|N\/A )\] (.+)$/gm)]
  .map((m) => ({ status: m[1], name: m[2] }));
const wasmView = report.checks.map((c) => ({ status: c.status === "pass" ? "PASS" : c.status.toUpperCase(), name: c.name }));
const same = JSON.stringify(cli) === JSON.stringify(wasmView);
console.log(same ? "SAME five results as the CLI" : `DIFFERENT from the CLI:\n${JSON.stringify(cli)}\n${JSON.stringify(wasmView)}`);
process.exit(same && report.passed ? 0 : 1);
