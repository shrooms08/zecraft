import type { RoundSummary } from "../api";
import { proofPath } from "../api";
import { shortHash, taz } from "../format";
import { VerifyRound } from "./VerifyRound";

const STATUS: Record<RoundSummary["status"], string> = {
  paid: "paid",
  paying: "paying",
  no_win: "carried over",
  voided: "voided, refunded",
};

/** Settled rounds. Deliberately shows no winner and no entry: only totals. */
export function PastRounds({ rounds }: { rounds: RoundSummary[] | null }) {
  if (!rounds) return <p>Loading past rounds…</p>;
  if (rounds.length === 0) return <p>No round has settled with entries yet.</p>;
  return (
    <div className="past-rounds">
      <h3>Past rounds</h3>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">Round</th>
              <th scope="col">Status</th>
              <th scope="col">Tickets</th>
              <th scope="col">Pot (TAZ)</th>
              <th scope="col">Beacon round</th>
              <th scope="col">Winning ticket</th>
              <th scope="col">Payout transaction</th>
              <th scope="col">Proof</th>
            </tr>
          </thead>
          <tbody>
            {rounds.map((r) => (
              <tr key={r.round}>
                <th scope="row">{r.round}</th>
                <td>{STATUS[r.status]}</td>
                <td>{r.game.tickets_sold}</td>
                <td>{taz(r.game.pot_zat)}</td>
                <td>{r.status === "voided" ? "not used" : r.drand_round}</td>
                <td>{r.outcome.winning_ticket ?? "none"}</td>
                <td className="mono">{r.payout_txid ? <span title={r.payout_txid}>{shortHash(r.payout_txid)}</span> : "none"}</td>
                <td>
                  <a href={proofPath(r.round)}>JSON</a>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <h3>Verify</h3>
      <p>
        Recheck a round yourself: your browser fetches its blocks and transactions through the relay and runs the five checks in
        WebAssembly, including the payout transaction's zero-knowledge proof. It takes up to a minute.
      </p>
      <ul className="verify-list">
        {rounds.map((r) => (
          <li key={r.round}>
            <VerifyRound round={r.round} />
          </li>
        ))}
      </ul>
    </div>
  );
}
