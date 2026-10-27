import { GITHUB_URL } from "../links";
import { HeroAnimation } from "./HeroAnimation";

const FLOW = ["entries", "vault", "drand", "2 of 3 sign", "payout"];

export function Hero() {
  return (
    <div className="hero">
      <div className="hero-copy">
        <div className="live-pill">
          <span className="live-dot" aria-hidden="true" />
          <span>Live on Zcash testnet</span>
        </div>
        <h1>
          The private game engine
          <br />
          <span className="accent">for Zcash.</span>
        </h1>
        <p className="hero-lede">Build a fair, private game on Zcash in a weekend.</p>
        <p className="hero-small">Open source. Extracted from the live game Zeek.</p>
        <div className="hero-actions">
          <a className="btn btn-primary btn-shadow" href={GITHUB_URL}>
            Build a game <span aria-hidden="true">→</span>
          </a>
          <a className="btn btn-ghost" href="#raffle">
            Watch a live round <span aria-hidden="true">↓</span>
          </a>
        </div>
      </div>
      <div className="hero-art-col">
        <div className="panel hero-panel">
          <div className="hero-panel-top">
            <span>round loop</span>
            <span>6 s</span>
          </div>
          <div role="img" aria-label="Shielded entries fall into a vault, a drand beacon pulses, two of three vault keys light up, and one coin goes to a sealed address.">
            <HeroAnimation />
          </div>
          <div className="hero-flow">
            {FLOW.map((s, i) => (
              <span key={s}>
                {i > 0 && <span className="dim" aria-hidden="true">→ </span>}
                {s}
              </span>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
