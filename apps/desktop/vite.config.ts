import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ command }) => ({
  base: command === "serve" ? "/" : "./",
  plugins: [
    react(),
    {
      name: "dst-world-options-watch",
      configureServer(server) {
        // A generated dependency may not exist on the first import. Vite only
        // auto-watches external files after resolving them successfully.
        server.watcher.add(`${server.config.root}/../../modules/dontstarve/world-options.json`);
      }
    }
  ],
  optimizeDeps: {
    // HLS already ships standalone ESM. Load it lazily without a generated
    // optimizer entry that can become stale while the desktop stays open.
    exclude: ["hls.js"]
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Language catalogs, mock data, and WebGL stay behind independent lazy boundaries.
    // Keep a finite desktop capability-chunk budget so future growth still surfaces.
    chunkSizeWarningLimit: 1_900,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: "react-vendor",
              test: /node_modules[\\/](?:react(?:-dom)?|scheduler)(?:[\\/]|$)/,
              priority: 20
            },
            {
              name: "tauri-vendor",
              test: /node_modules[\\/]@tauri-apps[\\/]/,
              priority: 20
            }
          ]
        }
      }
    }
  }
}));
