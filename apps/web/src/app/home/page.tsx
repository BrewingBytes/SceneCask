import type { Metadata } from "next";
import { AppShell } from "../../components/shell";
import { HomeView } from "../../features/home";

// Home sections come from client data after an authorized fetch; the server renders no account data.
export const metadata: Metadata = { title: "Home · SceneCask", referrer: "no-referrer" };

export default function HomePage() {
  return (
    <AppShell currentPath="/home" homeHref="/home">
      <HomeView />
    </AppShell>
  );
}
