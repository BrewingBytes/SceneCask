import type { Metadata } from "next";
import { Welcome } from "../features/home";

export const metadata: Metadata = { title: "SceneCask · Keep track of your shows" };

export default function WelcomePage() {
  return <Welcome />;
}
