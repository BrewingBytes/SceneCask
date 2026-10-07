import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  testMatch: "foundation.spec.ts",
  outputDir: "../../../../../../test-results/r04",
  use: { baseURL: "http://127.0.0.1:3104", browserName: "chromium" },
  webServer: {
    command: "node gallery-server.mjs",
    url: "http://127.0.0.1:3104",
    reuseExistingServer: !process.env.CI,
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120000,
  },
});
