import { GITHUB_URL } from "../links";
import { LogoTile } from "./Logo";

export function Footer() {
  return (
    <footer className="wrap footer">
      <div className="footer-inner">
        <span className="footer-line">
          <LogoTile size={22} />
          Built with Zecraft: an open-source kit for fair, private games on Zcash.
        </span>
        <a href={GITHUB_URL} className="mono">
          github.com/shrooms08/zecraft
        </a>
      </div>
    </footer>
  );
}
