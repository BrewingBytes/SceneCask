import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "discover.spec.ts",
  outputDir: "../../../../../../test-results/r19",
  use: { baseURL: "http://127.0.0.1:3119", browserName: "chromium" },
  webServer: {
    command: "node fixture-server.mjs",
    url: "http://127.0.0.1:3119/discover",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
