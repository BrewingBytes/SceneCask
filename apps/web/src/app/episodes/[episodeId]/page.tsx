import type { Metadata } from "next";
import { PrivateShell } from "../../_integration/private-shell";
import { EpisodeDetail } from "../../../features/catalog/episode";

// The server renders no episode data: details arrive only from an authorized client fetch, and
// the browser title is set from the show name and episode code, never the protected title.
export const metadata: Metadata = { title: "Episode · SceneCask", referrer: "no-referrer" };

export default async function EpisodePage({ params }: PageProps<"/episodes/[episodeId]">) {
  const { episodeId } = await params;
  return (
    <PrivateShell currentPath="/episodes">
      <EpisodeDetail key={episodeId} episodeId={episodeId} />
    </PrivateShell>
  );
}
