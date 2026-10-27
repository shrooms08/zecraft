# Zecraft page (games/raffle/web)

The Zecraft landing page and its live example game, the testnet raffle. Vite +
React + TypeScript, no styling framework. The engine serves the build at `/`
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
round checks (`verifyPlan`, `verifyScan`, `verifyRound`, in a Web Worker),
including the payout transaction's Halo2 proof and signatures.

## For the designer

Everything visual is in `src/styles.css` (one plain stylesheet, CSS variables
at the top, light and dark). Components are small and semantic; restyle them
by class name, or edit their markup:

| File | What it renders |
|---|---|
| `src/styles.css` | All styling |
| `index.html` | Page title and meta description |
| `src/App.tsx` | Section order (no markup of its own besides `<main>`) |
| `src/components/Hero.tsx` | `header.hero`: title, two taglines, the two buttons, testnet badge |
| `src/components/EngineFeatures.tsx` | "What the engine gives you": 4 `.card`s |
| `src/components/WriteYourGame.tsx` | "Write your game": the Rules trait in `pre.code` (text comes from `src/rulesSnippet.ts`) |
| `src/components/RaffleSection.tsx` | "Built with Zecraft: live on testnet" and the "Raffle" sub-heading |
| `src/components/CurrentRound.tsx` | Round number, status and the `.stats` grid |
| `src/components/HowToEnter.tsx` | Steps, vault address, the entry form and the built request |
| `src/components/PastRounds.tsx` | Past rounds table and the Verify list |
| `src/components/VerifyRound.tsx` | Verify button, progress line, the five checks |
| `src/components/GamesBuilt.tsx` | "Games built with Zecraft": 3 `.card`s |
| `src/components/PrivacyNote.tsx` | `aside.privacy` (exact wording; do not edit the text) |
| `src/components/Footer.tsx` | Footer line and GitHub link |
| `src/components/Section.tsx`, `CopyButton.tsx`, `QrCode.tsx` | Shared bits: section wrapper, copy button, QR image |

Leave alone: `src/api.ts`, `src/wasm.ts`, `src/verify.worker.ts`,
`src/format.ts`, `src/rulesSnippet.ts` (data, wasm and formatting).
