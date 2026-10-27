# Zecraft page (games/raffle/web)

The Zecraft landing page and its live example game, the testnet raffle. Vite +
React + TypeScript, no styling framework. The design source is
`design/Zecraft.html` at the repo root (a self-contained export; open it in a
browser). The engine serves the build at `/`
(`ZECRAFT_WEB_DIR`) and the chain relay at `/relay`, so the page talks to one
origin.

```sh
npm install
npm run wasm     # scripts/build-wasm.sh: games/raffle/wasm -> src/wasm/ (stable toolchain)
npm run build    # dist/, served by the engine (scripts/local/up.sh)
npm run dev      # Vite dev server, proxying /v1 and /relay to ZECRAFT_ENGINE_URL
node scripts/check-page.mjs http://127.0.0.1:7787 <a testnet UA> ../../../docs/screenshots
```

In the browser the wasm module seals entries (`buildEntry`) and runs the five
round checks (`verifyPlan`, `v## Layout

Everything visual is in `src/styles.css` (dark theme; tokens at the top:
`--accent #409EFE`, `--green`, `--red`, Geist / Geist Mono / Silkscreen from
Google Fonts, linked in `index.html`). All motion stops under
`prefers-reduced-motion`. One small component per section, in page order:

| File | What it renders |
|---|---|
| `index.html` | Title, meta, SVG favicon, fonts |
| `src/App.tsx` | Data (board, rounds, pending rounds, config, every 10 s) and section order |
| `src/components/Nav.tsx`, `Logo.tsx` | Nav; the Z mark (tile and bare) |
| `src/components/Hero.tsx`, `HeroAnimation.tsx` | Hero; the pixel vault loop (SVG + CSS keyframes `ha-*`) |
| `src/components/ProofStrip.tsx` | Round 0 facts; "Verify round 0 yourself" starts the verifier |
| `src/components/ThreeFunctions.tsx` | The real Rules trait (`src/rulesSnippet.ts`) and the `zecraft new` terminal |
| `src/components/ProblemLine.tsx` | The one-line problem statement |
| `src/components/HowItWorks.tsx` | The six-step timeline (round 0's values) |
| `src/components/EngineFeatures.tsx` | The bento: verification checks and three features |
| `src/components/WhatYouCanBuild.tsx` | Use-case tiles |
| `src/components/RaffleSection.tsx` | The live raffle: `CurrentRound`, `HowToEnter`, `PastRounds`, `VerifyRound` |
| `src/components/PrivacyNote.tsx` | The privacy statement (exact wording; do not edit the text) |
| `src/components/GamesBuilt.tsx` | Cartridges: raffle, coin flip, Zeek, your game |
| `src/components/CtaBlock.tsx`, `Footer.tsx` | Closing call to action; footer |
| `src/components/Pixel.tsx`, `CopyButton.tsx`, `QrCode.tsx` | Pixel icons, copy button, QR image |

Every live number comes from the engine API. Past rounds come from
`/v1/rounds` (rounds with entries) and `/v1/rounds/pending`: a settled round
the list leaves out had no entries, and consecutive ones collapse into one row.

Leave alone: `src/api.ts`, `src/wasm.ts`, `src/verify.worker.ts`,
`src/format.ts`, `src/rulesSnippet.ts` (data, wasm and formatting).
