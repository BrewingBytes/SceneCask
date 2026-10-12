import { createApiTransport } from "../search/api";

/**
 * Browser adapter for episode reads and C07 `episode_details` reveals. One per viewer; it caches
 * only the CSRF token. Progress writes go through the shared tracking client instead.
 */
export function createEpisodeApi() {
  const { client, request } = createApiTransport();
  return {
    getEpisode: (episodeId: string) => request(() => client.GET("/episodes/{id}", { params: { path: { id: episodeId } } })),
    reveal: (episodeId: string) =>
      request(() => client.POST("/reveals", { body: { scope: "episode_details", resourceId: episodeId } }), undefined, true),
    hide: (episodeId: string) =>
      request(
        async () => {
          const result = await client.DELETE("/reveals/{scope}/{resourceId}", {
            params: { path: { scope: "episode_details", resourceId: episodeId } },
            body: {},
          });
          // 204 has no body; the status alone is the success.
          return { ...result, data: result.response.ok ? null : undefined };
        },
        undefined,
        true,
      ),
  };
}

export type EpisodeApi = ReturnType<typeof createEpisodeApi>;
