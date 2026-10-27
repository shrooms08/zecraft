import { pixelRuns } from "./Pixel";

// One round as a 6 s loop: entries fall into the vault, the drand beacon
// pulses, two of three keys light up and a coin flies to a sealed address.
// The motion is CSS (styles.css, `ha-*`); under prefers-reduced-motion the
// same classes hold a still frame.
const BEACON = ["....####....", "..##....##..", ".#........#.", ".#...gg...#.", ".#...gg...#.", "..##....##..", "....####....", ".....##.....", ".....##.....", "....####....", "...######..."];
const LOCK = [".###.", "#...#", "#####", "##.##", "##.##", "#####"];
const COIN = [".gg.", "gggg", "gggg", ".gg."];
const DIAL = [".###.", "#...#", "#.g.#", "#...#", ".###."];

const T = "var(--text)";
const M = "var(--muted)";
const G = "var(--accent)";
const L = "var(--line)";
const P2 = "var(--panel2)";
const BG = "var(--bg)";

export function HeroAnimation() {
  const beam = [];
  for (let y = 17; y < 29; y += 3) beam.push(<rect key={y} x={27} y={y} width={2} height={2} fill={G} />);
  return (
    <svg className="hero-art" viewBox="0 0 80 66" width="100%" shapeRendering="crispEdges" aria-hidden="true">
      <rect className="ha-pulse1" x={20.5} y={1.5} width={15} height={14} fill="none" stroke={G} strokeWidth={0.6} />
      <rect className="ha-pulse2" x={17.5} y={-1.5} width={21} height={20} fill="none" stroke={G} strokeWidth={0.4} />
      <g>{pixelRuns(BEACON, 22, 3, { "#": T, g: G }, "bc")}</g>
      <text className="ha-pixel" x={40} y={7} fill={M} fontSize={2.8}>
        DRAND
      </text>
      <g className="ha-beam">{beam}</g>
      {[15, 36, 15].map((x, i) => (
        <g key={i} className={`ha-lock ha-lock${i}`}>
          {pixelRuns(LOCK, x, 24, { "#": G }, `l${i}`)}
        </g>
      ))}
      <rect x={10} y={30} width={36} height={30} fill={G} />
      <rect x={11} y={31} width={34} height={28} fill={P2} />
      <rect x={14} y={30} width={28} height={1} fill={BG} />
      <rect x={14} y={34} width={28} height={14} fill={L} />
      <rect x={15} y={35} width={26} height={12} fill={P2} />
      <g>{pixelRuns(DIAL, 26, 39, { "#": G, g: T }, "d")}</g>
      {[16, 25, 34].map((x, i) => (
        <rect key={x} className={`ha-key ${i < 2 ? `ha-key${i}` : ""}`} x={x} y={51} width={6} height={4} />
      ))}
      <rect x={12} y={60} width={4} height={2} fill={G} />
      <rect x={40} y={60} width={4} height={2} fill={G} />
      <text className="ha-pixel" x={10} y={65.5} fill={M} fontSize={2.8}>
        VAULT 2 OF 3
      </text>
      <text className="ha-pixel" x={52} y={44.5} fill={M} fontSize={2.6}>
        SEALED
      </text>
      <rect x={52} y={46} width={26} height={10} fill={P2} stroke={L} strokeWidth={0.5} />
      <text className="ha-mono" x={54} y={52.5} fill={T} fontSize={3.4}>
        utest1
      </text>
      <text className="ha-mono ha-blur" x={65.5} y={52.5} fill={T} fontSize={3.4}>
        k9x…q
      </text>
      <g className="ha-coin">{pixelRuns(COIN, 63, 40, { g: G }, "c")}</g>
    </svg>
  );
}
