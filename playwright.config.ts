import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests/e2e",
  use: { baseURL: "http://127.0.0.1:3000", browserName: "chromium" },
  webServer: [
    { command: "cargo run --locked --manifest-path services/api/Cargo.toml", url: "http://127.0.0.1:8080/health/ready", timeout: 120000 },
    { command: "pnpm --filter @scenecask/web dev", url: "http://127.0.0.1:3000", timeout: 120000 },
  ],
});
