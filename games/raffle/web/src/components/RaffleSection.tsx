import type { Board, GameConfig, PendingRound, RoundSummary } from "../api";
import { CurrentRound } from "./CurrentRound";
import { HowToEnter } from "./HowToEnter";
import { PastRounds } from "./PastRounds";
import { PixelIcon } from "./Pixel";
import { VerifyRound, type VerifyRequest } from "./VerifyRound";

interface Props {
  board: Board | null;
  config: GameConfig | null;
  configJson: string | null;
  rounds: RoundSummary[] | null;
  pending: PendingRound[] | null;
  error: string | null;
  verify: VerifyRequest;
  onVerify: (round: number) => void;
  reduced: boolean;
}

export function RaffleSection({ board, config, configJson, rounds, pending, error, verify, onVerify, reduced }: Props) {
  return (
    <section id="raffle" className="wrap section" aria-labelledby="raffle-h">
      <div className="eyebrow">proof</div>
      <h2 id="raffle-h" className="h2 h2-tight">
        Built with Zecraft: live on testnet
      </h2>
      <h3 className="raffle-sub">
        <PixelIcon name="ticket" size={22} />
        Raffle
      </h3>
      {error && (
        <p className="error engine-error" role="alert">
          The engine is not answering ({error}).
        </p>
      )}
      <div className="raffle-grid">
        <CurrentRound board={board} config={config} />
        <HowToEnter config={config} configJson={configJson} board={board} />
      </div>
      <PastRounds rounds={rounds} pending={pending} board={board} onVerify={onVerify} />
      <VerifyRound request={verify} reduced={reduced} />
    </section>
  );
}
