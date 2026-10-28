import type { Board, PendingRound, RoundSummary } from "../api";
import { proofPath } from "../api";
import { shortHash, taz } from "../format";

const STATUS: Record<RoundSummary["status"], { text: string; tone: string }> = {
  paid: { text: "paid", tone: "good" },
  paying: { text: "paying", tone: "accent" },
  no_win: { text: "carried over", tone: "muted" },
  voided: { text: "voided, refunded", tone: "bad" },
};

/** The engine's list (`/v1/rounds`) has only rounds with entries. */
const LIST_LIMIT = 50;

type Item = { kind: "round"; r: RoundSummary } | { kind: "empty"; lo: number; hi: number };

/**
 * Settled rounds, newest first. A settled round the list leaves out had no
 * entries; consecutive ones collapse into one row. Rounds at or above the
 * settled frontier (the oldest pending round, else the current one) are
 * not settled yet and are left out.
 */
function items(rounds: RoundSummary[], pending: PendingRound[] | null, board: Board | null): Item[] {
  const listed = new Map(rounds.map((r) => [r.round, r]));
  const known = [...(pending ?? []).map((p) => p.round), ...(board ? [board.round] : [])];
  const frontier = known.length ? Math.min(...known) : rounds.length ? Math.max(...rounds.map((r) => r.round)) + 1 : 0;
  // A full page may stop short of round 0: only fill gaps inside it.
  const lo = rounds.length >= LIST_LIMIT ? Math.min(...rounds.map((r) => r.round)) : 0;
  const out: Item[] = [];
  for (let n = frontier - 1; n >= lo; n--) {
    const r = listed.get(n);
    if (r) {
      out.push({ kind: "round", r });
      continue;
    }
    const last = out[out.length - 1];
    if (last?.kind === "empty" && last.lo === n + 1) last.lo = n;
    else out.push({ kind: "empty", lo: n, hi: n });
  }
  return out;
}

interface Props {
  rounds: RoundSummary[] | null;
  pending: PendingRound[] | null;
  board: Board | null;
  onVerify: (round: number) => void;
}

/** Deliberately shows no winner and no entry: only totals. */
export function PastRounds({ rounds, pending, board, onVerify }: Props) {
  const list = rounds ? items(rounds, pending, board) : null;
  return (
    <div className="panel past-rounds" data-component="PastRounds">
      <div className="panel-head">
        <span className="label">past rounds</span>
        <span className="muted small">Winners are never shown.</span>
      </div>
      {!list ? (
        <div className="rounds-list" aria-busy="true">
          <div className="skeleton sk-row" />
          <div className="skeleton sk-row" />
          <span className="mono muted small">Loading past rounds...</span>
        </div>
      ) : list.length === 0 ? (
        <p className="placeholder">No round has settled yet. The first one settles a few minutes after it closes.</p>
      ) : (
        <ul className="rounds-list">
          {list.map((it) =>
            it.kind === "empty" ? (
              <li key={`e${it.lo}`} className="round-empty" data-rounds={`${it.lo}-${it.hi}`}>
                <span className="mono">{it.lo === it.hi ? `Round ${it.lo}` : `Rounds ${it.lo}–${it.hi}`}: no entries</span>
              </li>
            ) : (
              <RoundRow key={it.r.round} r={it.r} onVerify={onVerify} />
            ),
          )}
        </ul>
      )}
    </div>
  );
}

/** A round whose entries were all refunded carried nothing over. */
function statusOf(r: RoundSummary) {
  if (r.status === "no_win" && r.refunds > 0 && r.game.tickets_sold === 0) {
    return { text: "refunded", tone: "muted" };
  }
  return STATUS[r.status];
}

function RoundRow({ r, onVerify }: { r: RoundSummary; onVerify: (round: number) => void }) {
  const status = statusOf(r);
  const fields: [string, string, string?][] = [
    ["round", String(r.round)],
    ["tickets", String(r.game.tickets_sold)],
    ["pot", `${taz(r.game.pot_zat)} TAZ`],
    ["beacon", r.status === "voided" ? "not used" : String(r.drand_round)],
    ["win ticket", r.outcome.winning_ticket === null ? "none" : String(r.outcome.winning_ticket)],
    ["payout txid", r.payout_txid ? shortHash(r.payout_txid) : "none", r.payout_txid ?? undefined],
  ];
  return (
    <li className="round-row" data-round={r.round} data-status={r.status}>
      <dl>
        <div>
          <dt>{fields[0][0]}</dt>
          <dd className="strong">{fields[0][1]}</dd>
        </div>
        <div>
          <dt>status</dt>
          <dd className={`tone-${status.tone}`}>{status.text}</dd>
        </div>
        {fields.slice(1).map(([k, v, title]) => (
          <div key={k} className={k === "payout txid" ? "txid" : undefined}>
            <dt>{k}</dt>
            <dd title={title}>{v}</dd>
          </div>
        ))}
      </dl>
      <div className="round-actions">
        <a className="btn btn-outline btn-sm" href={proofPath(r.round)}>
          Proof <span aria-hidden="true">→</span>
        </a>
        {r.status !== "paying" && (
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => onVerify(r.round)} aria-label={`Verify round ${r.round}`}>
            Verify
          </button>
        )}
      </div>
    </li>
  );
}
