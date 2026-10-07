import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "overlays.spec.ts",
  outputDir: "../../../../../../test-results/r05",
  use: { baseURL: "http://127.0.0.1:3105", browserName: "chromium" },
  webServer: {
    command: "node gallery-server.mjs",
    url: "http://127.0.0.1:3105",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
