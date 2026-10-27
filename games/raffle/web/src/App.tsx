import { useEffect, useState } from "react";
import { getBoard, getConfigText, getPending, getRounds, type Board, type GameConfig, type PendingRound, type RoundSummary } from "./api";
import { CtaBlock } from "./components/CtaBlock";
import { EngineFeatures } from "./components/EngineFeatures";
import { Footer } from "./components/Footer";
import { GamesBuilt } from "./components/GamesBuilt";
import { Hero } from "./components/Hero";
import { HowItWorks } from "./components/HowItWorks";
import { Nav } from "./components/Nav";
import { PrivacyNote } from "./components/PrivacyNote";
import { ProblemLine } from "./components/ProblemLine";
import { ProofStrip } from "./components/ProofStrip";
import { RaffleSection } from "./components/RaffleSection";
import { ThreeFunctions } from "./components/ThreeFunctions";
import type { VerifyRequest } from "./components/VerifyRound";
import { WhatYouCanBuild } from "./components/WhatYouCanBuild";
import { useReducedMotion } from "./hooks";

const REFRESH_MS = 10_000;

export function App() {
  const [board, setBoard] = useState<Board | null>(null);
  const [rounds, setRounds] = useState<RoundSummary[] | null>(null);
  const [pending, setPending] = useState<PendingRound[] | null>(null);
  const [configJson, setConfigJson] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [verify, setVerify] = useState<VerifyRequest>({ round: 0, nonce: 0 });
  const reduced = useReducedMotion();

  useEffect(() => {
    getConfigText().then(setConfigJson, (e) => setError(String(e)));
  }, []);

  useEffect(() => {
    let live = true;
    const tick = async () => {
      try {
        const [b, r, p] = await Promise.all([getBoard(), getRounds(), getPending()]);
        if (!live) return;
        setBoard(b);
        setRounds(r);
        setPending(p);
        setError(null);
      } catch (e) {
        if (live) setError(e instanceof Error ? e.message : String(e));
      }
    };
    tick();
    const id = setInterval(tick, REFRESH_MS);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, []);

  function verifyRound(round: number) {
    setVerify((v) => ({ round, nonce: v.nonce + 1 }));
    document.getElementById("verify")?.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
  }

  const config: GameConfig | null = configJson ? (JSON.parse(configJson) as GameConfig) : null;
  return (
    <div className="zc" id="top">
      <header className="wrap">
        <Nav />
        <Hero />
      </header>
      <main>
        <ProofStrip onVerify={() => verifyRound(0)} />
        <ThreeFunctions />
        <ProblemLine />
        <HowItWorks />
        <EngineFeatures />
        <WhatYouCanBuild />
        <RaffleSection
          board={board}
          config={config}
          configJson={configJson}
          rounds={rounds}
          pending={pending}
          error={error}
          verify={verify}
          onVerify={verifyRound}
          reduced={reduced}
        />
        <PrivacyNote />
        <GamesBuilt />
        <CtaBlock />
      </main>
      <Footer />
    </div>
  );
}
