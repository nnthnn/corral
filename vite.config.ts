import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// @xterm/xterm resolves to its esbuild ESM build (lib/xterm.mjs). An earlier
// esbuild-minifier bug constant-folded that build's `requestMode` enum
// (`let r; (V => (V[...] = ...))(r ||= {})`) into `void 0 || (r = {})` and
// dropped the `let r;`, throwing a ReferenceError on opencode's DECRQM mode
// query (`CSI ? Ps $ p`). That only happened under esbuild minification; this
// build runs Vite 8's Rolldown/Oxc pipeline, which preserves the declaration
// (`requestMode(e,t){let n;(e=>…)(n||={});` in the bundled output), so the old
// alias to the larger, webpack-built CJS bundle (lib/xterm.js) is no longer
// needed — dropping it shrinks the xterm chunk by ~150 kB.

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  build: {
    rollupOptions: {
      // Two entries: the app shell and the browser window's chrome page.
      input: {
        main: new URL("./index.html", import.meta.url).pathname,
        browser: new URL("./browser.html", import.meta.url).pathname,
      },
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (
            id.includes("/react/") ||
            id.includes("/react-dom/") ||
            id.includes("/scheduler/")
          ) {
            return "react";
          }
          if (id.includes("/@xterm/")) return "xterm";
          if (id.includes("/@tauri-apps/")) return "tauri";
          return undefined;
        },
      },
    },
  },
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
