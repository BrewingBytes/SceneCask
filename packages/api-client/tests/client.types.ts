import { createSceneCaskClient, type components } from "../src/index.js";

const client = createSceneCaskClient({ getCsrfToken: () => "csrf" });

async function exerciseContract() {
  const result = await client.GET("/episodes/{id}", { params: { path: { id: "episode-id" } } });
  if (result.data?.detailsAccess === "locked") {
    // @ts-expect-error The locked union branch cannot expose protected details.
    result.data.details.title;
  } else if (result.data) {
    const title: string | null = result.data.details.title;
    void title;
  }
  await client.PUT("/progress/episodes/{episodeId}", {
    params: { path: { episodeId: "episode-id" }, header: { "Idempotency-Key": "action-id" } },
    body: { watched: true, expectedRevision: 0 },
  });
  // @ts-expect-error Progress mutations still require a per-action idempotency key.
  await client.PUT("/progress/episodes/{episodeId}", { params: { path: { episodeId: "episode-id" } }, body: { watched: true, expectedRevision: 0 } });
  await client.POST("/reveals", { body: { scope: "discussion", resourceId: "discussion-id" } });
  await client.DELETE("/me/identities/google", { body: {} });
  // @ts-expect-error Bodyless DELETE mutations now require an EmptyRequest JSON body.
  await client.DELETE("/me/identities/google");
  await client.POST("/shows/{id}/catch-up/preview", { params: { path: { id: "show-id" } }, body: { throughEpisodeId: "episode-id" } });
  // @ts-expect-error No invented routes.
  await client.GET("/playback");
  const status: components["schemas"]["LibraryStatus"] = "watching";
  // @ts-expect-error Completed is computed, never a manual library status.
  const invalidStatus: components["schemas"]["LibraryStatus"] = "completed";
  void [status, invalidStatus];
}

void exerciseContract;
