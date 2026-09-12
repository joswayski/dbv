import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  root: "ui",
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 1421,
    strictPort: true,
    // Amp orbs expose the dev server through generated portal hostnames.
    allowedHosts: process.env.AMP_ORB ? true : undefined,
  },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    globals: true,
  },
});
