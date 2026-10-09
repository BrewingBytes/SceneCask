import type { Metadata } from "next";
import { ProfileOnboarding } from "../../../features/auth";

export const metadata: Metadata = { title: "Set up your profile · SceneCask", referrer: "no-referrer" };

export default function ProfileOnboardingPage() {
  return <ProfileOnboarding />;
}
