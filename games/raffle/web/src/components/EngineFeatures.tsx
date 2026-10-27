import { useReducedMotion, useTicker } from "../hooks";
import { PixelIcon, type IconName } from "./Pixel";

const CHECKS = ["Beacon", "Entries", "Timing", "Settlement", "Payout"];
const FEATURES: [IconName, string, string][] = [
  ["beacon", "Fair draws", "drand randomness, published only after a round closes."],
  ["lock", "Private entries and payouts", "Entries in shielded memos, winners paid to sealed fresh addresses."],
  ["key", "Threshold vault", "2-of-3 FROST keyholders, each re-checking the result before signing."],
];

export function EngineFeatures() {
  const reduced = useReducedMotion();
  // One check passes every 0.7 s, then the list resets (7 s).
  const t = useTicker(700, !reduced);
  const lit = reduced ? CHECKS.length : Math.min(CHECKS.length, t % 10);
  return (
    <section className="wrap section" aria-labelledby="engine-h">
      <div className="eyebrow">the engine</div>
      <h2 id="engine-h" className="h2">
        What you get for free
      </h2>
      <div className="bento">
        <div className="panel bento-main">
          <PixelIcon name="check" />
          <div>
            <h3>Anyone can verify</h3>
            <p>Five checks, in the browser or the CLI.</p>
          </div>
          <ul className="mini-checks" aria-label="Verification checks">
            {CHECKS.map((name, i) => (
              <li key={name} className={i < lit ? "on" : undefined}>
                <span className="sq" aria-hidden="true" />
                <span className="name">{name}</span>
                <span className="badge">{i < lit ? "pass" : "..."}</span>
              </li>
            ))}
          </ul>
        </div>
        <div className="bento-side">
          {FEATURES.map(([icon, title, desc]) => (
            <div key={title} className="panel feature">
              <PixelIcon name={icon} />
              <div>
                <h3>{title}</h3>
                <p>{desc}</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
