// Round 0's facts, as published in the design: fixed, not live.
const PROOF = ["1 live round settled on testnet", "5/5 checks pass", "Verified in your browser in 10.9 s", "2 games, 0 engine changes", "157 tests"];

export function ProofStrip({ onVerify }: { onVerify: () => void }) {
  return (
    <section className="proof-strip" aria-label="Proof">
      <div className="wrap proof-inner">
        <ul>
          {PROOF.map((p) => (
            <li key={p}>
              <span className="sq" aria-hidden="true" />
              {p}
            </li>
          ))}
        </ul>
        <button type="button" className="btn btn-outline" onClick={onVerify}>
          Verify round 0 yourself
        </button>
      </div>
    </section>
  );
}
