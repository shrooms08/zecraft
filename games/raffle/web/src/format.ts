export const COIN = 100_000_000;

/** Zatoshis as TAZ, trailing zeros trimmed: 585000 -> "0.00585". */
export function taz(zat: number): string {
  const whole = Math.floor(zat / COIN);
  const frac = String(zat % COIN).padStart(8, "0").replace(/0+$/, "");
  return frac ? `${whole}.${frac}` : String(whole);
}

/** Seconds as "about 9 min" / "under a minute". */
export function aboutMinutes(secs: number): string {
  if (secs < 60) return "under a minute";
  return `about ${Math.round(secs / 60)} min`;
}

export function shortHash(hex: string): string {
  return hex.length > 16 ? `${hex.slice(0, 8)}…${hex.slice(-8)}` : hex;
}
