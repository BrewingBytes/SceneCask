import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "episode.spec.ts",
  outputDir: "../../../../../../test-results/r22",
  use: { baseURL: "http://127.0.0.1:3123", browserName: "chromium" },
  webServer: {
    command: "node fixture-server.mjs",
    url: "http://127.0.0.1:3123/shows/ready",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
