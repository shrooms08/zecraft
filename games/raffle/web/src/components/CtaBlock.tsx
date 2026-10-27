import { BUILD_GUIDE_URL } from "../links";
import { CopyButton } from "./CopyButton";
import { LogoMark } from "./Logo";

export function CtaBlock() {
  return (
    <section className="wrap section" aria-labelledby="cta-h">
      <div className="panel cta">
        <div className="cta-copy">
          <LogoMark size={48} />
          <h2 id="cta-h">
            Build a fair, private game <span className="accent">this weekend.</span>
          </h2>
        </div>
        <div className="cta-actions">
          <a className="btn btn-primary btn-shadow btn-block" href={BUILD_GUIDE_URL}>
            Build a game <span aria-hidden="true">→</span>
          </a>
          <div className="cmd-box">
            <code>
              <span className="tok-prompt">$</span> zecraft new mygame
            </code>
            <CopyButton text="zecraft new mygame" />
          </div>
        </div>
      </div>
    </section>
  );
}
