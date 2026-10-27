import type { ReactElement } from "react";

/**
 * Pixel art as SVG rects: each row is a string, `.` is empty and any other
 * character is a pixel coloured by `colors`. Horizontal runs merge into one rect.
 */
export function pixelRuns(rows: string[], ox: number, oy: number, colors: Record<string, string>, key: string): ReactElement[] {
  const out: ReactElement[] = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const c = row[x];
      if (c === ".") {
        x++;
        continue;
      }
      let w = 1;
      while (row[x + w] === c) w++;
      out.push(<rect key={`${key}${x}_${y}`} x={ox + x} y={oy + y} width={w} height={1} fill={colors[c] ?? "currentColor"} />);
      x += w;
    }
  });
  return out;
}

/** `#` is the text colour, `g` the accent. */
export const ICONS = {
  lock: ["..####..", ".#....#.", ".#....#.", "########", "###gg###", "###gg###", "####.###", "########"],
  ticket: ["##########", "#..#.....#", "...#.####.", "...#......", "...#.###..", "#..#.....#", "##########"],
  coin: ["..####..", ".##..##.", "##.gg.##", "##.gg.##", "##.gg.##", "##.gg.##", ".##..##.", "..####.."],
  grid: ["##.##.##", "##.##.##", "........", "##.gg.##", "##.gg.##", "........", "##.##.##", "##.##.##"],
  ball: ["..####..", ".######.", "########", "##....##", "####.###", "###.####", ".##.###.", "..####.."],
  dice: ["########", "#......#", "#.g..g.#", "#......#", "#......#", "#.g..g.#", "#......#", "########"],
  box: [".######.", "#..##..#", "########", "#..gg..#", "#..gg..#", "#......#", "#......#", "########"],
  vault: ["########", "#......#", "#.####.#", "#.#gg#.#", "#.#gg#.#", "#.####.#", "#......#", "########"],
  beacon: ["#......#", ".#.gg.#.", "..#gg#..", "...##...", "...##...", "...##...", "..####..", ".######."],
  check: [".......g", "......gg", ".....gg.", "g...gg..", "gg.gg...", ".ggg....", "..g.....", "........"],
  key: [".###.....", "#...#....", "#.g.#####", "#...#.#.#", ".###....."],
  plus: ["...##...", "...##...", "...##...", "########", "########", "...##...", "...##...", "...##..."],
} satisfies Record<string, string[]>;

export type IconName = keyof typeof ICONS;

export function PixelIcon({ name, size = 32 }: { name: IconName; size?: number }) {
  const rows = ICONS[name];
  const w = rows[0].length;
  const h = rows.length;
  return (
    <svg className="px-icon" viewBox={`0 0 ${w} ${h}`} width={size} height={Math.round((size * h) / w)} shapeRendering="crispEdges" aria-hidden="true">
      {pixelRuns(rows, 0, 0, { "#": "currentColor", g: "var(--accent)" }, "p")}
    </svg>
  );
}
