import type { Metadata } from "next";
import { AppShell } from "../../components/shell";
import { DiscoverSearch } from "../../features/catalog/search";

export const metadata: Metadata = { title: "Discover · SceneCask", referrer: "no-referrer" };

export default async function DiscoverPage({ searchParams }: PageProps<"/discover">) {
  const { q } = await searchParams;
  return (
    <AppShell currentPath="/discover">
      <DiscoverSearch initialQuery={typeof q === "string" ? q : ""} />
    </AppShell>
  );
}
