// The raffle's wasm module on the page's main thread (entry sealing only;
// verification runs in a worker so the page stays responsive).
import init, { buildEntry } from "./wasm/zecraft_raffle_wasm.js";

let ready: Promise<unknown> | null = null;

export interface Entry {
  uri: string;
  round: number;
  tickets: number;
  amount_zat: number;
  memo: string;
}

export async function sealEntry(
  configJson: string,
  round: number,
  tickets: number,
  payoutAddress: string,
  textMemo: boolean,
): Promise<Entry> {
  ready ??= init();
  await ready;
  return JSON.parse(buildEntry(configJson, round, tickets, payoutAddress, textMemo)) as Entry;
}
