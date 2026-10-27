import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The engine serves `dist/` at its own origin, so every API path is
// relative. `npm run dev` proxies them to a local engine.
const engine = process.env.ZECRAFT_ENGINE_URL ?? "http://127.0.0.1:7787";

export default defineConfig({
  plugins: [react()],
  build: { outDir: "dist", target: "es2022", assetsInlineLimit: 0 },
  worker: { format: "es" },
  server: {
    // The Rules trait snippet is read from the kit's source.
    fs: { allow: ["../../.."] },
    proxy: { "/v1": engine, "/relay": engine },
  },
});
