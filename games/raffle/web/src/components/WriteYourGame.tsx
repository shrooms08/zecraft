import { Section } from "./Section";
import { rulesTrait } from "../rulesSnippet";

export function WriteYourGame() {
  return (
    <Section id="write" title="Write your game">
      <p>Implement three functions. Zecraft runs the rounds, the draw, the vault and the verification.</p>
      <figure>
        <pre className="code">
          <code>{rulesTrait}</code>
        </pre>
        <figcaption>
          From <code>crates/zecraft-core/src/rules.rs</code>
        </figcaption>
      </figure>
    </Section>
  );
}
