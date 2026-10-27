import { GITHUB_URL } from "../links";
import { LogoTile } from "./Logo";

export function Nav() {
  return (
    <nav className="nav" aria-label="Main">
      <a href="#top" className="brand">
        <LogoTile size={28} />
        <span>Zecraft</span>
      </a>
      <div className="nav-links">
        <a href="#raffle">Live raffle</a>
        <a href={GITHUB_URL}>GitHub</a>
      </div>
    </nav>
  );
}
