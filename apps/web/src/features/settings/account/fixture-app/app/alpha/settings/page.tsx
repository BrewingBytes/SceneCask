import { SettingsView } from "../../../../settings-view";

/** The same view with SCENECASK_BETA off: no social sections and no social requests. */
export default function AlphaSettings() {
  return (
    <main className="sc-foundation">
      <SettingsView beta={false} />
    </main>
  );
}
