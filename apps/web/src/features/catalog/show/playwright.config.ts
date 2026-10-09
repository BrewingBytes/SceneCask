import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "show.spec.ts",
  outputDir: "../../../../../../test-results/r21",
  use: { baseURL: "http://127.0.0.1:3122", browserName: "chromium" },
  webServer: {
    command: "node fixture-server.mjs",
    url: "http://127.0.0.1:3122/episodes/ready",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
