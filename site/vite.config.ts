import { defineConfig } from "vite";
import { spectrumHtml } from "./build/html.ts";

export default defineConfig({
  // Relative base so the build works under /<repo>/ on GitHub Pages.
  base: "./",
  plugins: [spectrumHtml()],
  build: {
    outDir: "dist",
    assetsInlineLimit: 1024,
    target: "es2022",
    cssCodeSplit: false,
    chunkSizeWarningLimit: 400,
  },
});
