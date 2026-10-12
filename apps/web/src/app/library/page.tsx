import type { Metadata } from "next";
import { PrivateShell } from "../_integration/private-shell";
import { LibraryView, parseStatus } from "../../features/library";

// Library rows come from client data after an authorized fetch; the server renders no library content.
export const metadata: Metadata = { title: "Library · SceneCask", referrer: "no-referrer" };

export default async function LibraryPage({ searchParams }: PageProps<"/library">) {
  const { status, q } = await searchParams;
  return (
    <PrivateShell currentPath="/library">
      <LibraryView
        initialQuery={{ status: parseStatus(typeof status === "string" ? status : null), q: typeof q === "string" ? q : "" }}
      />
    </PrivateShell>
  );
}
