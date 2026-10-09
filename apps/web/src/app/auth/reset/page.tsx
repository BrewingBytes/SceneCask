import type { Metadata } from "next";
import { ResetRequest } from "../../../features/auth";

export const metadata: Metadata = { title: "Reset your password · SceneCask", referrer: "no-referrer" };

export default function ResetPage() {
  return <ResetRequest />;
}
