import { createSceneCaskClient, type components } from "../src/index.js";

const client = createSceneCaskClient();

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
    params: { path: { episodeId: "episode-id" }, header: { Origin: "https://example.test", "X-CSRF-Token": "csrf", "Idempotency-Key": "action-id" } },
    body: { watched: true, expectedRevision: 0 },
  });
  // @ts-expect-error Progress mutations must include Origin, CSRF and idempotency headers.
  await client.PUT("/progress/episodes/{episodeId}", { params: { path: { episodeId: "episode-id" } }, body: { watched: true, expectedRevision: 0 } });
  // @ts-expect-error No invented routes.
  await client.GET("/playback");
  const status: components["schemas"]["LibraryStatus"] = "watching";
  // @ts-expect-error Completed is computed, never a manual library status.
  const invalidStatus: components["schemas"]["LibraryStatus"] = "completed";
  void [status, invalidStatus];
}

void exerciseContract;
