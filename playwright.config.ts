import { defineConfig } from "@playwright/test";
import { OIDC_CLIENT_ID, OIDC_CLIENT_SECRET, OIDC_ISSUER, TMDB_BASE, TMDB_TOKEN } from "./tests/e2e/alpha-fixtures/data.mjs";
export default defineConfig({
  testDir: "./tests/e2e",
  use: { baseURL: "http://127.0.0.1:3000", browserName: "chromium" },
  webServer: [
    // TMDB/Google stand-ins at the provider boundary (R24); everything else is the real stack.
    { command: "node tests/e2e/alpha-fixtures/server.mjs", url: `${OIDC_ISSUER}/.well-known/openid-configuration`, timeout: 30000 },
    {
      command: "cargo run --locked --manifest-path services/api/Cargo.toml",
      url: "http://127.0.0.1:8080/health/ready",
      timeout: 120000,
      env: {
        PUBLIC_ORIGIN: "http://127.0.0.1:3000",
        API_MIGRATE_ON_START: "true",
        TMDB_API_TOKEN: TMDB_TOKEN,
        TMDB_FIXTURE_BASE: TMDB_BASE,
        GOOGLE_CLIENT_ID: OIDC_CLIENT_ID,
        GOOGLE_CLIENT_SECRET: OIDC_CLIENT_SECRET,
        GOOGLE_FIXTURE_ISSUER: OIDC_ISSUER,
      },
    },
    { command: "yarn workspace @scenecask/web dev", url: "http://127.0.0.1:3000", timeout: 120000 },
  ],
});
