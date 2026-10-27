import type { Board } from "../api";
import { aboutMinutes, taz } from "../format";

const STATUS: Record<Board["status"], string> = {
  open: "open for tickets",
  closing: "closed, waiting for the blocks after it",
  revealable: "closed, waiting for the beacon",
  settled: "settled",
  voided: "voided (refunded)",
};

export function CurrentRound({ board, spacing }: { board: Board | null; spacing: number }) {
  if (!board) return <p>Loading the board…</p>;
  const g = board.game;
  return (
    <div className="current-round">
      <h3>
        Round {board.round} <span className="status">{STATUS[board.status]}</span>
      </h3>
      <dl className="stats">
        <div>
          <dt>Blocks left</dt>
          <dd>
            {board.blocks_remaining} <small>({aboutMinutes(board.blocks_remaining * spacing)})</small>
          </dd>
        </div>
        <div>
          <dt>Tickets sold</dt>
          <dd>{g.tickets_sold}</dd>
        </div>
        <div>
          <dt>Entries</dt>
          <dd>{g.entries}</dd>
        </div>
        <div>
          <dt>Pot</dt>
          <dd>{taz(g.pot_zat + board.carry)} TAZ</dd>
        </div>
        <div>
          <dt>Ticket price</dt>
          <dd>{taz(g.ticket_price_zat)} TAZ</dd>
        </div>
      </dl>
      <p className="note">
        Chain tip {board.tip_height}. The board refreshes every 10 seconds.
      </p>
    </div>
  );
}
