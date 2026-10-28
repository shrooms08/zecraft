import { useRef, useState } from "react";
import type { Board, GameConfig } from "../api";
import { taz } from "../format";
import { sealEntry, type Entry } from "../wasm";
import { CopyButton } from "./CopyButton";
import { QrCode } from "./QrCode";

/** Blocks a purchase needs left in the window (zecraft-player's margin + 1). */
const MIN_BLOCKS_LEFT = 4;

export function HowToEnter({ config, configJson, board }: { config: GameConfig | null; configJson: string | null; board: Board | null }) {
  return (
    <div className="panel how-to-enter" data-component="HowToEnter">
      <div className="label">how to enter</div>
      {!config || !configJson ? (
        <p className="muted small">Loading the game config...</p>
      ) : !config.entries_open || !config.vault_address ? (
        <p className="muted">Entries are closed on this engine.</p>
      ) : (
        <Steps config={config} configJson={configJson} board={board} vault={config.vault_address} />
      )}
    </div>
  );
}

function Steps({ config, configJson, board, vault }: { config: GameConfig; configJson: string; board: Board | null; vault: string }) {
  const [tickets, setTickets] = useState("2");
  const [payout, setPayout] = useState("");
  const [textMemo, setTextMemo] = useState(false);
  const [entry, setEntry] = useState<Entry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const price = config.game.ticket_price_zat;
  const n = Math.max(1, Math.min(1000, Math.floor(Number(tickets)) || 1));
  const closing = !board || board.status !== "open" || board.blocks_remaining < MIN_BLOCKS_LEFT;

  // A payment request belongs to the inputs it was built from: any edit
  // withdraws it until "Create payment" builds a new one, and a request
  // still being sealed when an edit happens is dropped when it arrives.
  const inputs = useRef(0);
  function edit<T>(set: (value: T) => void, value: T) {
    inputs.current += 1;
    setEntry(null);
    set(value);
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!board) return;
    setBusy(true);
    setError(null);
    setEntry(null);
    const built = inputs.current;
    try {
      const sealed = await sealEntry(configJson, board.round, n, payout.trim(), textMemo);
      if (inputs.current === built) setEntry(sealed);
    } catch (e) {
      if (inputs.current === built) setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const cliPayout = payout.trim() || "<your utest1 address>";
  const cliRound = entry?.round ?? board?.round ?? "<round>";
  const cli = `zecraft-player buy <name> --tickets ${n} --round ${cliRound} --payout-address ${cliPayout}${textMemo ? " --text-memo" : ""}`;

  return (
    <ol className="enter-steps">
      <li>
        <span className="step-num" aria-hidden="true">1</span>
        <div className="step-body">
          <span className="step-head">The vault address</span>
          <code className="addr">{vault}</code>
          <p className="warn small">Pay with the payment link in step 3. A payment without its memo can't be refunded.</p>
        </div>
      </li>
      <li>
        <span className="step-num" aria-hidden="true">2</span>
        <form className="step-body" onSubmit={submit}>
          <span className="step-head">Choose your tickets</span>
          <div className="form-row">
            <div className="field field-tickets">
              <label htmlFor="zc-tickets">Tickets</label>
              <input id="zc-tickets" type="number" min={1} max={1000} value={tickets} required onChange={(e) => edit(setTickets, e.target.value)} />
            </div>
            <div className="field field-payout">
              <label htmlFor="zc-payout">Your payout address (fresh, goes in the shielded memo)</label>
              <input id="zc-payout" type="text" value={payout} required placeholder="utest1..." spellCheck={false} autoComplete="off" onChange={(e) => edit(setPayout, e.target.value)} />
            </div>
            <button type="submit" className="btn btn-primary" disabled={busy || closing}>
              {busy ? "Sealing..." : "Create payment"}
            </button>
          </div>
          <label className="checkbox">
            <input type="checkbox" checked={textMemo} onChange={(e) => edit(setTextMemo, e.target.checked)} />
            Text memo (for wallets that only send text memos)
          </label>
          <span className="mono muted small">
            Total: {taz(price * n)} TAZ ({n} × {taz(price)})
          </span>
          {closing && board && (
            <p className="warn small" role="status">
              Round {board.round} is {board.status === "open" ? `closing in ${board.blocks_remaining} blocks` : "closed"}: wait for the next round to open.
            </p>
          )}
          {error && (
            <p className="error small" role="alert">
              {error}
            </p>
          )}
        </form>
      </li>
      <li>
        <span className="step-num" aria-hidden="true">3</span>
        <div className="step-body">
          <span className="step-head">Pay the vault</span>
          <div aria-live="polite">
            {entry ? (
              <div className="entry-request">
                <div className="entry-text">
                  <span>
                    Send <strong className="mono">{taz(entry.amount_zat)} TAZ</strong> for {entry.tickets} ticket{entry.tickets === 1 ? "" : "s"} in round {entry.round}. The link includes your memo.
                  </span>
                  <a className="btn btn-primary btn-sm" href={entry.uri}>
                    Open payment link
                  </a>
                  <code className="entry-uri" title={entry.uri}>
                    {entry.uri.slice(0, 22)}…{entry.uri.slice(-18)}
                  </code>
                  <CopyButton text={entry.uri} label="Copy payment link" />
                </div>
                <QrCode value={entry.uri} label="QR code of the payment request" size={148} />
              </div>
            ) : (
              <p className="placeholder">Fill in step 2 to get a payment link and QR.</p>
            )}
          </div>
          <p className="muted small">Pay from an NU7 testnet wallet or zecraft-player.</p>
          <details className="cli">
            <summary>Use the CLI instead</summary>
            <pre>
              <span className="tok-prompt">$</span> {cli}
            </pre>
          </details>
        </div>
      </li>
    </ol>
  );
}
