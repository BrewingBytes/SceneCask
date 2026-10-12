// Shared by the fixture servers, the Playwright config and the alpha journeys.
export const TMDB_PORT = 4010;
export const OIDC_PORT = 4011;
export const TMDB_TOKEN = "alpha-fixture-tmdb-token";
export const OIDC_CLIENT_ID = "alpha-fixture-client";
export const OIDC_CLIENT_SECRET = "alpha-fixture-secret";
export const TMDB_BASE = `http://127.0.0.1:${TMDB_PORT}/3/`;
export const OIDC_ISSUER = `http://127.0.0.1:${OIDC_PORT}`;

const episode = (id, number, title, airDate) => ({ id, number, title, airDate });

/** Episode titles double as protected-content sentinels: none may reach a viewer who has not
 * watched (or revealed) that episode, in JSON, HTML or any other response. */
export const CATALOG = [
  {
    id: 9001,
    name: "Hollow Orchard",
    firstAirDate: "2024-02-01",
    genreIds: [18],
    synopsis: "A family orchard keeps its secrets through one long winter.",
    status: "Returning Series",
    seasons: [
      { id: 90010, number: 0, episodes: [episode(900100, 1, "Orchard Letters", "2024-06-01")] },
      {
        id: 90011,
        number: 1,
        episodes: [
          episode(900101, 1, "Quiet Harvest", "2024-02-01"),
          episode(900102, 2, "Lantern Debt", "2024-02-08"),
          episode(900103, 3, "The Cellar Door", "2024-02-15"),
          episode(900104, 4, "Ash in the Rows", "2024-02-22"),
          episode(900105, 5, "Last Frost", "2099-01-01"),
        ],
      },
    ],
  },
  {
    id: 9002,
    name: "Hollow Orchard",
    firstAirDate: "2010-09-04",
    genreIds: [35],
    synopsis: "An unrelated sitcom with the same name.",
    status: "Ended",
    seasons: [{ id: 90021, number: 1, episodes: [episode(900201, 1, "Pilot Pie", "2010-09-04")] }],
  },
];

export const PROTECTED_TITLES = CATALOG.flatMap((show) =>
  show.seasons.flatMap((season) => season.episodes.map((e) => e.title)),
);
