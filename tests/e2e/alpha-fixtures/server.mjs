// Provider-boundary fixtures for the alpha browser journeys. The real API talks to these exactly
// as it would to TMDB and Google; nothing in the browser or API is mocked. Fixture-only data and
// credentials: never point a deployment at these servers.
import { createHash, createSign, generateKeyPairSync, randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { CATALOG, PROTECTED_TITLES, TMDB_PORT, OIDC_PORT, OIDC_CLIENT_ID, OIDC_CLIENT_SECRET, TMDB_TOKEN } from "./data.mjs";

const json = (response, status, body) => {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(body));
};

async function bodyOf(request) {
  let text = "";
  for await (const chunk of request) text += chunk;
  return text;
}

// ---------------------------------------------------------------- TMDB (https://api.themoviedb.org/3/)

let tmdbDown = false;

function season(show, number) {
  const summary = show.seasons.find((s) => s.number === number);
  return {
    id: summary.id,
    season_number: number,
    episodes: summary.episodes.map((episode) => ({
      id: episode.id,
      season_number: number,
      episode_number: episode.number,
      name: episode.title,
      overview: `${episode.title} overview`,
      still_path: `/still-${episode.id}.jpg`,
      air_date: episode.airDate,
    })),
  };
}

function tmdb(request, response) {
  const url = new URL(request.url, "http://fixture");
  if (url.pathname === "/__control" && request.method === "POST") {
    return void bodyOf(request).then((text) => {
      tmdbDown = Boolean(JSON.parse(text || "{}").down);
      json(response, 200, { down: tmdbDown });
    });
  }
  if (request.headers.authorization !== `Bearer ${TMDB_TOKEN}`) return json(response, 401, { status_code: 7 });
  if (tmdbDown) return json(response, 503, { status_code: 43 });
  const path = url.pathname.replace(/^\/3\//, "");
  if (path === "configuration") {
    return json(response, 200, { images: { secure_base_url: "https://image.tmdb.org/t/p/", poster_sizes: ["w500"] } });
  }
  if (path === "genre/tv/list") return json(response, 200, { genres: [{ id: 18, name: "Drama" }, { id: 35, name: "Comedy" }] });
  if (path === "search/tv") {
    const q = (url.searchParams.get("query") ?? "").toLowerCase();
    const results = CATALOG.filter((show) => show.name.toLowerCase().includes(q)).map((show) => ({
      id: show.id,
      name: show.name,
      first_air_date: show.firstAirDate,
      genre_ids: show.genreIds,
      poster_path: null,
    }));
    return json(response, 200, { page: 1, total_pages: 1, results });
  }
  const showMatch = /^tv\/(\d+)$/.exec(path);
  const seasonMatch = /^tv\/(\d+)\/season\/(\d+)$/.exec(path);
  const show = CATALOG.find((s) => s.id === Number((showMatch ?? seasonMatch)?.[1]));
  if (!show) return json(response, 404, { status_code: 34 });
  if (seasonMatch) return json(response, 200, season(show, Number(seasonMatch[2])));
  return json(response, 200, {
    id: show.id,
    name: show.name,
    first_air_date: show.firstAirDate,
    genres: show.genreIds.map((id) => ({ id, name: id === 18 ? "Drama" : "Comedy" })),
    overview: show.synopsis,
    poster_path: null,
    status: show.status,
    number_of_seasons: show.seasons.filter((s) => s.number > 0).length,
    number_of_episodes: show.seasons.filter((s) => s.number > 0).reduce((n, s) => n + s.episodes.length, 0),
    seasons: show.seasons.map((s) => ({ id: s.id, season_number: s.number, episode_count: s.episodes.length })),
  });
}

// ---------------------------------------------------------------- OIDC (accounts.google.com)

const issuer = `http://127.0.0.1:${OIDC_PORT}`;
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const kid = "alpha-fixture";
const jwk = { ...publicKey.export({ format: "jwk" }), kid, alg: "RS256", use: "sig" };
const b64url = (value) => Buffer.from(value).toString("base64url");
/** The identity the next authorization signs in as; set by the test through /__identity. */
let identity = null;
const codes = new Map();

function idToken(claims) {
  const input = `${b64url(JSON.stringify({ alg: "RS256", typ: "JWT", kid }))}.${b64url(JSON.stringify(claims))}`;
  const signature = createSign("RSA-SHA256").update(input).sign(privateKey);
  return `${input}.${b64url(signature)}`;
}

function oidc(request, response) {
  const url = new URL(request.url, issuer);
  if (url.pathname === "/.well-known/openid-configuration") {
    return json(response, 200, {
      issuer,
      authorization_endpoint: `${issuer}/authorize`,
      token_endpoint: `${issuer}/token`,
      jwks_uri: `${issuer}/jwks`,
      response_types_supported: ["code"],
      subject_types_supported: ["public"],
      id_token_signing_alg_values_supported: ["RS256"],
    });
  }
  if (url.pathname === "/jwks") return json(response, 200, { keys: [jwk] });
  if (url.pathname === "/__identity" && request.method === "POST") {
    return void bodyOf(request).then((text) => {
      identity = JSON.parse(text);
      json(response, 200, {});
    });
  }
  if (url.pathname === "/authorize") {
    const params = url.searchParams;
    const redirect = new URL(params.get("redirect_uri"));
    redirect.searchParams.set("state", params.get("state"));
    if (!identity) {
      redirect.searchParams.set("error", "access_denied");
    } else {
      const code = randomUUID();
      codes.set(code, { identity, nonce: params.get("nonce"), challenge: params.get("code_challenge"), redirect: params.get("redirect_uri") });
      redirect.searchParams.set("code", code);
    }
    response.writeHead(302, { location: redirect.toString() });
    return void response.end();
  }
  if (url.pathname === "/token" && request.method === "POST") {
    return void bodyOf(request).then((text) => {
      const form = new URLSearchParams(text);
      const grant = codes.get(form.get("code"));
      codes.delete(form.get("code"));
      const basic = `Basic ${Buffer.from(`${OIDC_CLIENT_ID}:${OIDC_CLIENT_SECRET}`).toString("base64")}`;
      const verifier = form.get("code_verifier") ?? "";
      const pkce = grant && createHash("sha256").update(verifier).digest("base64url") === grant.challenge;
      if (!grant || !pkce || request.headers.authorization !== basic || form.get("redirect_uri") !== grant.redirect) {
        return json(response, 400, { error: "invalid_grant" });
      }
      const now = Math.floor(Date.now() / 1000);
      json(response, 200, {
        access_token: "fixture-access-token",
        token_type: "Bearer",
        expires_in: 3600,
        id_token: idToken({
          iss: issuer,
          aud: OIDC_CLIENT_ID,
          sub: grant.identity.sub,
          email: grant.identity.email,
          email_verified: true,
          name: grant.identity.name,
          nonce: grant.nonce,
          auth_time: now,
          iat: now,
          exp: now + 300,
        }),
      });
    });
  }
  json(response, 404, {});
}

if (PROTECTED_TITLES.length === 0) throw new Error("fixture catalog has no protected titles");
createServer(tmdb).listen(TMDB_PORT, "127.0.0.1");
createServer(oidc).listen(OIDC_PORT, "127.0.0.1");
