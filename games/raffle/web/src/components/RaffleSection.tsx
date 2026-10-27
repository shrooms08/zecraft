import type { Board, GameConfig, RoundSummary } from "../api";
import { CurrentRound } from "./CurrentRound";
import { HowToEnter } from "./HowToEnter";
import { PastRounds } from "./PastRounds";
import { Section } from "./Section";

interface Props {
  board: Board | null;
  config: GameConfig | null;
  configJson: string | null;
  rounds: RoundSummary[] | null;
  error: string | null;
}

export function RaffleSection({ board, config, configJson, rounds, error }: Props) {
  const spacing = config?.chain.target_spacing_secs ?? 25;
  return (
    <Section id="live" title="Built with Zecraft: live on testnet">
      <article id="raffle" aria-labelledby="raffle-title" className="raffle">
        <h3 id="raffle-title" className="subheading">
          Raffle
        </h3>
        <p>
          Buy tickets with a shielded payment. When the round closes, a drand beacon published afterwards draws one ticket,
          and the vault's keyholders pay its holder the whole pot.
        </p>
        {error && (
          <p className="error" role="alert">
            The engine is not answering ({error}).
          </p>
        )}
        <CurrentRound board={board} spacing={spacing} />
        <HowToEnter config={config} configJson={configJson} board={board} />
        <PastRounds rounds={rounds} />
      </article>
    </Section>
  );
}
