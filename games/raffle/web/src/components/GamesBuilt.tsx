import { COINFLIP_URL, ZEEK_URL } from "../links";
import { CopyButton } from "./CopyButton";
import { PixelIcon, type IconName } from "./Pixel";

const CARTS: { icon: IconName; name: string; desc: string; cta: string; href: string; live?: boolean; self?: boolean }[] = [
  { icon: "ticket", name: "Raffle", desc: "Live on testnet. This page.", cta: "See the round ↑", href: "#raffle", live: true, self: true },
  { icon: "coin", name: "Coin flip", desc: "Example game in the repo.", cta: "Read the code →", href: COINFLIP_URL },
  { icon: "grid", name: "Zeek", desc: "The 5x5 dig game Zecraft was extracted from.", cta: "Play Zeek →", href: ZEEK_URL },
];

export function GamesBuilt() {
  return (
    <section className="wrap section" aria-labelledby="games-h">
      <div className="eyebrow">games</div>
      <h2 id="games-h" className="h2">
        Built on the same engine
      </h2>
      <ul className="carts">
        {CARTS.map((g) => (
          <li key={g.name} className="cart">
            <div className="cart-shell">
              <div className="cart-grip" aria-hidden="true" />
              <div className="cart-label">
                <div className="cart-top">
                  <PixelIcon name={g.icon} />
                  {g.live && <span className="tag tag-live">live</span>}
                </div>
                <div className="cart-text">
                  <h3>{g.name}</h3>
                  <p>{g.desc}</p>
                  <a href={g.href} className={g.self ? "cart-link self" : "cart-link"}>
                    {g.cta}
                  </a>
                </div>
              </div>
              <div className="cart-pins" aria-hidden="true" />
            </div>
          </li>
        ))}
        <li className="cart-new">
          <span className="dim">
            <PixelIcon name="plus" />
          </span>
          <div className="cart-new-text">
            <h3>Your game here</h3>
            <div className="cmd-box">
              <code>zecraft new mygame</code>
              <CopyButton text="zecraft new mygame" />
            </div>
          </div>
        </li>
      </ul>
    </section>
  );
}
