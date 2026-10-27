// The engine's HTTP API, same origin. Types cover only what the page reads.

export type RoundStatus = "open" | "closing" | "revealable" | "settled" | "voided";

export interface RaffleBoard {
  game: "raffle";
  ticket_price_zat: number;
  tickets_sold: number;
  entries: number;
  pot_zat: number;
  refunds: number;
}

export interface Board {
  tip_height: number;
  round: number;
  status: RoundStatus;
  window: { start: number; end: number };
  blocks_remaining: number;
  estimated_close_time: number;
  carry: number;
  entries: number;
  /** Mean spacing of the last 20 blocks the engine stored (null until known). */
  recent_spacing_secs: number | null;
  game: RaffleBoard;
}

export interface GameConfig {
  chain: { network: string; genesis_height: number; blocks_per_round: number; target_spacing_secs: number };
  game: { ticket_price_zat: number; entry_fee_zat: number; output_fee_zat: number };
  memo: { magic: string; version: number; text_prefix: string };
  entries_open: boolean;
  vault_address?: string;
  sealing_public_key?: string;
  uivk?: string;
}

export interface RaffleOutcome {
  tickets_sold: number;
  winning_ticket: number | null;
  winner_paid: boolean;
}

export interface RoundSummary {
  round: number;
  status: "paid" | "paying" | "no_win" | "voided";
  outcome: RaffleOutcome;
  game: RaffleBoard;
  drand_round: number;
  paid: number;
  wins: number;
  refunds: number;
  payout_txid: string | null;
}

async function json<T>(path: string): Promise<T> {
  const r = await fetch(path, { headers: { accept: "application/json" } });
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return (await r.json()) as T;
}

export const getBoard = () => json<Board>("/v1/board");
export const getConfig = () => json<GameConfig>("/v1/config");
export const getConfigText = async () => {
  const r = await fetch("/v1/config");
  if (!r.ok) throw new Error(`/v1/config: ${r.status}`);
  return r.text();
};
export const getRounds = () => json<{ rounds: RoundSummary[] }>("/v1/rounds?limit=50").then((r) => r.rounds);
export const proofPath = (round: number) => `/v1/rounds/${round}/proof`;

/** A closed round the engine has not settled yet. */
export interface PendingRound {
  round: number;
  status: string;
}
export const getPending = () => json<{ rounds: PendingRound[] }>("/v1/rounds/pending").then((r) => r.rounds);
