import type { Metadata } from "next";
import { AppShell } from "../../../components/shell";
import { ShowDetail } from "../../../features/catalog/show";

// Titles come from client data after an authorized fetch; the server never renders show content.
export const metadata: Metadata = { title: "Show · SceneCask", referrer: "no-referrer" };

export default async function ShowPage({ params, searchParams }: PageProps<"/shows/[showId]">) {
  const { showId } = await params;
  const { season } = await searchParams;
  const requested = typeof season === "string" && /^\d{1,3}$/.test(season) ? Number(season) : undefined;
  return (
    <AppShell currentPath="/shows">
      <ShowDetail key={showId} showId={showId} initialSeason={requested} />
    </AppShell>
  );
}
