import { useEffect, useState } from "react";
import { getBoard, getConfigText, getRounds, type Board, type GameConfig, type RoundSummary } from "./api";
import { EngineFeatures } from "./components/EngineFeatures";
import { Footer } from "./components/Footer";
import { GamesBuilt } from "./components/GamesBuilt";
import { Hero } from "./components/Hero";
import { PrivacyNote } from "./components/PrivacyNote";
import { RaffleSection } from "./components/RaffleSection";
import { WriteYourGame } from "./components/WriteYourGame";

/** Placeholder until the repository is public. */
export const GITHUB_URL = "https://github.com/zecraft/zecraft";
const REFRESH_MS = 10_000;

export function App() {
  const [board, setBoard] = useState<Board | null>(null);
  const [rounds, setRounds] = useState<RoundSummary[] | null>(null);
  const [configJson, setConfigJson] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getConfigText().then(setConfigJson, (e) => setError(String(e)));
  }, []);

  useEffect(() => {
    let live = true;
    const tick = async () => {
      try {
        const [b, r] = await Promise.all([getBoard(), getRounds()]);
        if (!live) return;
        setBoard(b);
        setRounds(r);
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

  const config: GameConfig | null = configJson ? (JSON.parse(configJson) as GameConfig) : null;
  return (
    <>
      <Hero githubUrl={GITHUB_URL} />
      <main>
        <EngineFeatures />
        <WriteYourGame />
        <RaffleSection board={board} config={config} configJson={configJson} rounds={rounds} error={error} />
        <GamesBuilt githubUrl={GITHUB_URL} />
        <PrivacyNote />
      </main>
      <Footer githubUrl={GITHUB_URL} />
    </>
  );
}
