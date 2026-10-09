import type { Metadata } from "next";
import { CredentialsForm } from "../../../features/auth";

export const metadata: Metadata = { title: "Create your account · SceneCask", referrer: "no-referrer" };

export default function SignupPage() {
  return <CredentialsForm mode="signup" />;
}
