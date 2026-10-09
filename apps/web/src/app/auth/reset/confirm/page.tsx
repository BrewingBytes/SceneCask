import type { Metadata } from "next";
import { ResetConfirm } from "../../../../features/auth";

// The link's token is in the fragment; no-referrer keeps even the path out of outgoing requests.
export const metadata: Metadata = { title: "Choose a new password · SceneCask", referrer: "no-referrer" };

export default function ResetConfirmPage() {
  return <ResetConfirm />;
}
