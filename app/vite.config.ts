import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { readFileSync } from "node:fs";

const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));

// Port 1420 is what the Tauri config expects in dev. In a plain browser the
// /api calls are proxied to `stagewright serve` on 4870.
export default defineConfig({
  plugins: [svelte()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The API only answers its own origin; present proxied calls as same-origin.
    proxy: {
      "/api": {
        target: "http://127.0.0.1:4870",
        changeOrigin: true,
        configure: (proxy) => proxy.on("proxyReq", (req) => req.removeHeader("origin")),
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: { target: "es2022", sourcemap: true, chunkSizeWarningLimit: 1500 },
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
