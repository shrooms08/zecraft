export function Hero({ githubUrl }: { githubUrl: string }) {
  return (
    <header className="hero">
      <p className="badge">testnet</p>
      <h1>Zecraft</h1>
      <p className="lede">Build a fair, private game on Zcash in a weekend.</p>
      <p>
        An open-source game engine for shielded Zcash: provably fair draws, private payouts, a
        threshold vault and public round verification, extracted from the live game Zeek.
      </p>
      <nav className="hero-actions" aria-label="Get started">
        <a className="button primary" href="#raffle">
          Try the live raffle
        </a>
        <a className="button" href={githubUrl}>
          Build a game
        </a>
      </nav>
    </header>
  );
}
