import type { Metadata } from "next";
import { CredentialsForm, safeReturnTo } from "../../../features/auth";

export const metadata: Metadata = { title: "Sign in · SceneCask", referrer: "no-referrer" };

export default async function SigninPage({ searchParams }: PageProps<"/auth/signin">) {
  const { returnTo, error, reason } = await searchParams;
  return (
    <CredentialsForm
      mode="signin"
      returnTo={safeReturnTo(returnTo)}
      googleError={typeof error === "string" ? error : undefined}
      expired={reason === "expired"}
    />
  );
}
