import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "settings.spec.ts",
  outputDir: "../../../../../../test-results/r29",
  use: { baseURL: "http://127.0.0.1:3125", browserName: "chromium" },
  webServer: {
    command: "node fixture-server.mjs",
    url: "http://127.0.0.1:3125/auth/signin",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
