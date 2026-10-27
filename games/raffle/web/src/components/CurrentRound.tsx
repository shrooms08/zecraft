import type { Board, GameConfig } from "../api";
import { aboutMinutes, taz } from "../format";

const STATUS: Record<Board["status"], "open" | "closing" | "settling"> = {
  open: "open",
  closing: "closing",
  revealable: "settling",
  settled: "settling",
  voided: "settling",
};

export function CurrentRound({ board, config }: { board: Board | null; config: GameConfig | null }) {
  if (!board) {
    return (
      <div className="panel current-round" data-component="CurrentRound" aria-busy="true">
        <div className="label">current round</div>
        <div className="skeleton sk-title" />
        <div className="skeleton sk-countdown" />
        <div className="skeleton sk-stats" />
        <span className="mono muted small">Loading round from testnet...</span>
      </div>
    );
  }
  const g = board.game;
  const measured = board.recent_spacing_secs;
  const spacing = measured || config?.chain.target_spacing_secs || 25;
  const perRound = board.window.end - board.window.start;
  const elapsed = Math.max(0, Math.min(perRound, perRound - board.blocks_remaining));
  const status = STATUS[board.status];
  const pot = g.pot_zat + board.carry;
  const stats: [string, string][] = [
    ["Entries", String(g.entries)],
    ["Tickets sold", String(g.tickets_sold)],
    [board.carry > 0 ? "Pot (incl. carry)" : "Pot", `${taz(pot)} TAZ`],
    ["Ticket price", `${taz(g.ticket_price_zat)} TAZ`],
  ];
  return (
    <div className="panel current-round" data-component="CurrentRound">
      <div className="label">current round</div>
      <div className="round-head">
        <span className="round-title">Round {board.round}</span>
        <span className={`chip chip-${status}`}>{status}</span>
      </div>
      <div className="countdown" aria-live="polite">
        <div className="countdown-main">
          <span className="countdown-n">{board.blocks_remaining}</span>
          <span className="countdown-unit">{board.blocks_remaining === 1 ? "block left" : "blocks left"}</span>
        </div>
        <div className="countdown-sub">
          {aboutMinutes(board.blocks_remaining * spacing)} · {spacing} s per block{measured ? " (measured)" : " (target)"}
        </div>
        <div className="segments" role="progressbar" aria-label="Round window" aria-valuemin={0} aria-valuemax={perRound} aria-valuenow={elapsed}>
          {Array.from({ length: perRound }, (_, i) => (
            <span key={i} className={i < elapsed ? "on" : undefined} />
          ))}
        </div>
      </div>
      <dl className="stat-grid">
        {stats.map(([k, v]) => (
          <div key={k}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
      <p className="muted small">
        The draw uses a drand beacon published after the window closes and the reveal delay. One winner takes the pot.
      </p>
      <p className="mono dim xsmall">
        tip {board.tip_height} · window {board.window.start}–{board.window.end - 1} · refreshes every 10 s
      </p>
    </div>
  );
}
