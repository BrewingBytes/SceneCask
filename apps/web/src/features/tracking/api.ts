import type { components } from "@scenecask/api-client";
import { createApiTransport, type ApiResult } from "../catalog/search/api";

export { keyedUndo, newIdempotencyKey, outcomeUnknown, type ApiResult, type Failure } from "../catalog/search/api";

type Schemas = components["schemas"];
export type Show = Schemas["Show"];
export type Episode = Schemas["Episode"];
export type EpisodeDetails = Schemas["EpisodeDetails"];
export type Release = Schemas["Release"];
export type LibraryItem = Schemas["LibraryItem"];
export type LibraryStatus = Schemas["LibraryStatus"];
export type UndoResult = Schemas["UndoResult"];
export type CatchupPreview = Schemas["CatchupPreview"];
export type CatchupEpisode = Schemas["CatchupEpisode"];
export type SaveRequest = Schemas["SaveRequest"];

export type TrackingApi = ReturnType<typeof createTrackingApi>;
/** As the generated client returns it: the no-op branch omits actionId/undoUntil. */
export type MutationResult = Extract<Awaited<ReturnType<TrackingApi["setProgress"]>>, { ok: true }>["data"];
export const actionIdOf = (result: MutationResult) => ("actionId" in result ? result.actionId : null);

/** Episode pages are requested at the contract maximum and followed until the cursor ends. */
const EPISODE_PAGE = 50;

/** Browser adapter for show detail, progress, catch-up, history and undo. One per viewer. */
export function createTrackingApi() {
  const { client, request } = createApiTransport();
  const keyed = (key: string) => ({ "Idempotency-Key": key });

  async function listSeason(showId: string, season: number): Promise<ApiResult<Episode[]>> {
    const episodes: Episode[] = [];
    let cursor: string | undefined;
    do {
      const page = await request(() =>
        client.GET("/shows/{id}/episodes", {
          params: { path: { id: showId }, query: { season, limit: EPISODE_PAGE, cursor } },
        }),
      );
      if (!page.ok) return page;
      episodes.push(...page.data.items);
      cursor = page.data.nextCursor ?? undefined;
    } while (cursor);
    return { ok: true, data: episodes };
  }

  return {
    getShow: (showId: string) => request(() => client.GET("/shows/{id}", { params: { path: { id: showId } } })),
    listSeason,
    getEpisode: (episodeId: string) =>
      request(() => client.GET("/episodes/{id}", { params: { path: { id: episodeId } } })),
    setProgress: (episodeId: string, body: { watched: boolean; expectedRevision: number }, key: string) =>
      request(
        () =>
          client.PUT("/progress/episodes/{episodeId}", {
            params: { path: { episodeId }, header: keyed(key) },
            body,
          }),
        undefined,
        true,
      ),
    saveToLibrary: (showId: string, body: SaveRequest, key: string) =>
      request(
        () => client.PUT("/library/{showId}", { params: { path: { showId }, header: keyed(key) }, body }),
        undefined,
        true,
      ),
    setStatus: (showId: string, body: { status: LibraryStatus; expectedRevision: number }, key: string) =>
      request(
        () => client.PATCH("/library/{showId}", { params: { path: { showId }, header: keyed(key) }, body }),
        undefined,
        true,
      ),
    previewCatchup: (showId: string, throughEpisodeId: string) =>
      request(
        () => client.POST("/shows/{id}/catch-up/preview", { params: { path: { id: showId } }, body: { throughEpisodeId } }),
        undefined,
        true,
      ),
    commitCatchup: (showId: string, previewId: string, key: string) =>
      request(
        () =>
          client.POST("/shows/{id}/catch-up", {
            params: { path: { id: showId }, header: keyed(key) },
            body: { previewId },
          }),
        undefined,
        true,
      ),
    eraseHistory: (showId: string, expectedRevision: number, key: string) =>
      request(
        () =>
          client.DELETE("/shows/{id}/history", {
            params: { path: { id: showId }, header: keyed(key) },
            body: { expectedRevision },
          }),
        undefined,
        true,
      ),
    undo: (actionId: string, key: string) =>
      request(
        () => client.POST("/actions/{id}/undo", { params: { path: { id: actionId }, header: keyed(key) }, body: {} }),
        undefined,
        true,
      ),
  };
}
