import type { Metadata } from "next";
import { AppShell } from "../../../components/shell";
import { EpisodeDetail } from "../../../features/catalog/episode";

// The server renders no episode data: details arrive only from an authorized client fetch, and
// the browser title is set from the show name and episode code, never the protected title.
export const metadata: Metadata = { title: "Episode · SceneCask", referrer: "no-referrer" };

export default async function EpisodePage({ params }: PageProps<"/episodes/[episodeId]">) {
  const { episodeId } = await params;
  return (
    <AppShell currentPath="/episodes">
      <EpisodeDetail key={episodeId} episodeId={episodeId} />
    </AppShell>
  );
}
