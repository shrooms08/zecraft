import { PixelIcon, type IconName } from "./Pixel";

const BUILDS: { icon: IconName; title: string; desc: string; tag: string; live?: boolean }[] = [
  { icon: "ticket", title: "Raffles", desc: "Buy tickets, one winner takes the pot.", tag: "Live on testnet", live: true },
  { icon: "coin", title: "Coin flips", desc: "Pick a side, winners split the pot.", tag: "In the repo" },
  { icon: "grid", title: "Grid digs", desc: "Dig tiles, find treasure, like Zeek.", tag: "Zeek" },
  { icon: "ball", title: "Lotteries", desc: "Pick numbers, matches win.", tag: "zecraft new" },
  { icon: "dice", title: "Dice", desc: "Bet on a roll, fair by drand.", tag: "zecraft new" },
  { icon: "box", title: "Loot boxes", desc: "Pay in, a fair draw picks the prize.", tag: "zecraft new" },
];

export function WhatYouCanBuild() {
  return (
    <section className="wrap section" aria-labelledby="build-h">
      <div className="eyebrow">use cases</div>
      <h2 id="build-h" className="h2">
        What you can build
      </h2>
      <ul className="tiles">
        {BUILDS.map((b) => (
          <li key={b.title} className="panel tile">
            <div className="tile-top">
              <PixelIcon name={b.icon} />
              <span className={b.live ? "tag tag-live" : "tag"}>{b.tag}</span>
            </div>
            <div className="tile-text">
              <h3>{b.title}</h3>
              <p>{b.desc}</p>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
