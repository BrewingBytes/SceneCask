import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { betaEnabled, PrivateShell } from "../../_integration/private-shell";
import { ProfileView } from "../../../features/social";

// The profile arrives only from an authorized client fetch; names never reach server metadata.
export const metadata: Metadata = { title: "Profile · SceneCask", referrer: "no-referrer" };

export default async function PersonPage({ params }: PageProps<"/people/[id]">) {
  if (!(await betaEnabled())) notFound();
  const { id } = await params;
  return (
    <PrivateShell currentPath="/friends">
      <ProfileView key={id} personId={id} />
    </PrivateShell>
  );
}
