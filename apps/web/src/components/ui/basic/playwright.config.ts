import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: ".",
  globalTeardown: "./gallery-teardown.mjs",
  testMatch: "foundation.spec.ts",
  outputDir: "../../../../../../test-results/r04",
  use: { baseURL: "http://127.0.0.1:3104", browserName: "chromium" },
  webServer: {
    command:
      "yarn workspace @scenecask/web exec next dev src/components/ui/basic/gallery-app --hostname 127.0.0.1 --port 3104",
    url: "http://127.0.0.1:3104",
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
  },
});
