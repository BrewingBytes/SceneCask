import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "library.spec.ts",
  outputDir: "../../../../../test-results/r20",
  use: { baseURL: "http://127.0.0.1:3121", browserName: "chromium" },
  webServer: {
    command: "node fixture-server.mjs",
    url: "http://127.0.0.1:3121/discover",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
