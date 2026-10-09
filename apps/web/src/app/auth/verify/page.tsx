import type { Metadata } from "next";
import { VerifyEmail } from "../../../features/auth";

// The link's token is in the fragment; no-referrer keeps even the path out of outgoing requests.
export const metadata: Metadata = { title: "Verify your email · SceneCask", referrer: "no-referrer" };

export default function VerifyPage() {
  return <VerifyEmail />;
}
