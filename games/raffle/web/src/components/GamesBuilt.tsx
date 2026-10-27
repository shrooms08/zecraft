import { Section } from "./Section";

export function GamesBuilt({ githubUrl }: { githubUrl: string }) {
  return (
    <Section id="games" title="Games built with Zecraft">
      <ul className="cards">
        <li className="card">
          <h3>Raffle</h3>
          <p>
            Live on testnet: <a href="#raffle">this page</a>.
          </p>
        </li>
        <li className="card">
          <h3>Coin flip</h3>
          <p>
            Example game in the <a href={githubUrl}>repo</a>.
          </p>
        </li>
        <li className="card">
          <h3>Zeek</h3>
          <p>
            The 5x5 dig game Zecraft was extracted from: <a href="https://playzeek.pages.dev">playzeek.pages.dev</a>.
          </p>
        </li>
      </ul>
    </Section>
  );
}
