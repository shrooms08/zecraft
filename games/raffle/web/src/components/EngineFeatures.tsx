import { Section } from "./Section";

const FEATURES = [
  ["Fair draws", "drand randomness, published only after a round closes."],
  ["Private entries and payouts", "entries in shielded memos, winners paid to sealed fresh addresses."],
  ["Threshold vault", "2-of-3 FROST keyholders, each re-checking the result before signing."],
  ["Anyone can verify", "five checks, in the browser or the CLI."],
] as const;

export function EngineFeatures() {
  return (
    <Section id="engine" title="What the engine gives you">
      <ul className="cards">
        {FEATURES.map(([title, line]) => (
          <li key={title} className="card">
            <h3>{title}</h3>
            <p>{line}</p>
          </li>
        ))}
      </ul>
    </Section>
  );
}
