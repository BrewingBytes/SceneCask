import type { Metadata } from "next";
import { FriendRecommendations, FriendsActivity } from "../../../index";

export const metadata: Metadata = { title: "Social slots · SceneCask fixture" };

/** The Home and Discover beta slots on their own; R38 places them in the real routes. */
export default function FeedFixture() {
  return (
    <main className="sc-foundation sc-reading-column" style={{ padding: "0 20px 48px" }}>
      <FriendsActivity />
      <FriendRecommendations />
    </main>
  );
}
