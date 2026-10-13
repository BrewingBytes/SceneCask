import type { Metadata } from "next";
import { betaEnabled, PrivateShell } from "../_integration/private-shell";
import { SettingsView } from "../../features/settings/account";

// Account settings come from client data after an authorized fetch; the server renders no account data.
export const metadata: Metadata = { title: "Settings · SceneCask", referrer: "no-referrer" };

export default async function SettingsPage() {
  return (
    <PrivateShell currentPath="/settings">
      <SettingsView beta={await betaEnabled()} />
    </PrivateShell>
  );
}
