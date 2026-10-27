import { useReducedMotion, useTicker } from "../hooks";

// Round 0's real values, from its proof bundle.
const STEPS: [string, string][] = [
  ["Enter with a shielded memo", "3 entries, 6 tickets, block 4,547,147"],
  ["Window closes", "block 4,547,162"],
  ["drand beacon after the reveal delay", "drand round 33,431,631"],
  ["Rules settle", "winning ticket 2 of 6"],
  ["Keyholders re-check and sign (2 of 3)", "keyholders 1 + 2"],
  ["Anyone verifies", "5/5 PASS"],
];

export function HowItWorks() {
  const reduced = useReducedMotion();
  // Light one step every 0.8 s, hold, repeat (8.4 s).
  const t = useTicker(400, !reduced);
  const lit = reduced ? STEPS.length : Math.min(STEPS.length, Math.floor((t % 21) / 2));
  return (
    <section className="wrap section" aria-labelledby="how-h">
      <div className="section-head">
        <div>
          <div className="eyebrow">round 0, on testnet</div>
          <h2 id="how-h" className="h2 h2-tight">
            How a round works
          </h2>
        </div>
        <p className="aside">Every number below is from round 0.</p>
      </div>
      <ol className="timeline">
        {STEPS.map(([title, data], i) => (
          <li key={title} className={i < lit ? "on" : undefined}>
            <div className="track" aria-hidden="true">
              <span className="node" />
              <span className="bar" />
            </div>
            <div className="panel step">
              <span className="step-n">step {i + 1}</span>
              <span className="step-title">{title}</span>
              <span className="step-data">{data}</span>
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}
