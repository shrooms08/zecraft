// The Zecraft mark: a Z with a square dot.
const MARK = (
  <>
    <path d="M3 3H97V28L31 96H3V71L44 27H3Z" />
    <rect x="71" y="71" width="26" height="25" />
  </>
);

/** White mark on the blue rounded tile (nav, footer, favicon). */
export function LogoTile({ size = 28 }: { size?: number }) {
  return (
    <svg className="logo-tile" width={size} height={size} viewBox="0 0 28 28" aria-hidden="true">
      <rect width="28" height="28" rx="6" fill="#409EFE" />
      <g transform="translate(7 7) scale(0.14)" fill="#FFFFFF" stroke="#FFFFFF" strokeWidth="6" strokeLinejoin="round">
        {MARK}
      </g>
    </svg>
  );
}

/** The bare mark in the accent colour (CTA block). */
export function LogoMark({ size = 48 }: { size?: number }) {
  return (
    <svg className="logo-mark" width={size} height={size} viewBox="-3 -3 106 106" aria-hidden="true">
      <g fill="var(--accent)" stroke="var(--accent)" strokeWidth="6" strokeLinejoin="round">
        {MARK}
      </g>
    </svg>
  );
}
