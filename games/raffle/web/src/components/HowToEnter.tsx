import { useState } from "react";
import type { Board, GameConfig } from "../api";
import { taz } from "../format";
import { sealEntry, type Entry } from "../wasm";
import { CopyButton } from "./CopyButton";
import { QrCode } from "./QrCode";

/** Blocks a purchase needs left in the window (zecraft-player's margin + 1). */
const MIN_BLOCKS_LEFT = 4;

export function HowToEnter({ config, configJson, board }: { config: GameConfig | null; configJson: string | null; board: Board | null }) {
  if (!config || !configJson) return <p>Loading the game config…</p>;
  if (!config.entries_open || !config.vault_address) {
    return <p>Entries are closed on this engine.</p>;
  }
  return (
    <div className="how-to-enter">
      <h3>How to enter</h3>
      <ol className="steps">
        <li>Choose how many tickets ({taz(config.game.ticket_price_zat)} TAZ each).</li>
        <li>Paste a fresh shielded address of your own wallet: winnings go there.</li>
        <li>Pay the request below. The payout address is sealed in your browser, so only the vault's keyholders can open it, and only if you win.</li>
      </ol>
      <VaultAddress address={config.vault_address} />
      <EntryForm configJson={configJson} board={board} />
      <p className="note">Pay from an NU7 testnet wallet or zecraft-player.</p>
    </div>
  );
}

function VaultAddress({ address }: { address: string }) {
  return (
    <div className="vault-address">
      <h4>Vault address</h4>
      <p className="mono break">{address}</p>
      <CopyButton text={address} label="Copy address" />
      <QrCode value={address} label="QR code of the vault address" />
    </div>
  );
}

function EntryForm({ configJson, board }: { configJson: string; board: Board | null }) {
  const [tickets, setTickets] = useState(1);
  const [payout, setPayout] = useState("");
  const [textMemo, setTextMemo] = useState(false);
  const [entry, setEntry] = useState<Entry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const closing = !board || board.status !== "open" || board.blocks_remaining < MIN_BLOCKS_LEFT;

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!board) return;
    setBusy(true);
    setError(null);
    setEntry(null);
    try {
      setEntry(await sealEntry(configJson, board.round, tickets, payout.trim(), textMemo));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="entry-form" onSubmit={submit}>
      <h4>Build your entry</h4>
      <label>
        Tickets
        <input type="number" min={1} max={1000} value={tickets} required onChange={(e) => setTickets(Number(e.target.value))} />
      </label>
      <label>
        Your fresh payout address
        <input type="text" value={payout} required placeholder="utest1…" spellCheck={false} autoComplete="off" onChange={(e) => setPayout(e.target.value)} />
      </label>
      <label className="checkbox">
        <input type="checkbox" checked={textMemo} onChange={(e) => setTextMemo(e.target.checked)} />
        Text memo (for wallets that only send text memos)
      </label>
      {closing && board && <p className="warning">Round {board.round} closes in {board.blocks_remaining} blocks: wait for the next round.</p>}
      <button type="submit" className="button primary" disabled={busy || closing}>
        {busy ? "Sealing…" : `Build the request for round ${board?.round ?? "…"}`}
      </button>
      {error && <p className="error" role="alert">{error}</p>}
      {entry && <EntryRequest entry={entry} payout={payout.trim()} textMemo={textMemo} />}
    </form>
  );
}

function EntryRequest({ entry, payout, textMemo }: { entry: Entry; payout: string; textMemo: boolean }) {
  const cli = `zecraft-player buy <name> --tickets ${entry.tickets} --round ${entry.round} --payout-address ${payout}${textMemo ? " --text-memo" : ""}`;
  return (
    <div className="entry-request" aria-live="polite">
      <p>
        {entry.tickets} ticket{entry.tickets === 1 ? "" : "s"} for round {entry.round}: pay exactly {taz(entry.amount_zat)} TAZ.
      </p>
      <p className="mono break">{entry.uri}</p>
      <CopyButton text={entry.uri} label="Copy request" />
      <QrCode value={entry.uri} label="QR code of the payment request" />
      <details>
        <summary>Same entry with zecraft-player</summary>
        <p className="mono break">{cli}</p>
      </details>
    </div>
  );
}
