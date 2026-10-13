import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { betaEnabled, PrivateShell } from "../_integration/private-shell";
import { FriendsView, parseFriendsTab } from "../../features/social";

// People and relationships come from client data after an authorized fetch; the server renders none.
export const metadata: Metadata = { title: "Friends · SceneCask", referrer: "no-referrer" };

export default async function FriendsPage({ searchParams }: PageProps<"/friends">) {
  // Social is beta: with the flag off the API answers 404, and so does this route.
  if (!(await betaEnabled())) notFound();
  const { tab } = await searchParams;
  return (
    <PrivateShell currentPath="/friends">
      <FriendsView initialTab={parseFriendsTab(tab)} />
    </PrivateShell>
  );
}
