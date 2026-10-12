import type { Metadata } from "next";
import { PrivateShell } from "../_integration/private-shell";
import { DiscoverSearch } from "../../features/catalog/search";

export const metadata: Metadata = { title: "Discover · SceneCask", referrer: "no-referrer" };

export default async function DiscoverPage({ searchParams }: PageProps<"/discover">) {
  const { q } = await searchParams;
  return (
    <PrivateShell currentPath="/discover">
      <DiscoverSearch initialQuery={typeof q === "string" ? q : ""} />
    </PrivateShell>
  );
}
