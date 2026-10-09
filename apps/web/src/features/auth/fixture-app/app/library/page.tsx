"use client";
import { useEffect, useState } from "react";
import { endExpiredSession } from "../../../routes";

/**
 * Stand-in for a private screen (R24 owns the real ones). It holds the viewer's data in memory,
 * as real screens do, and hands a 401 to endExpiredSession.
 */
export default function FixtureLibrary() {
  const [titles, setTitles] = useState<string[]>([]);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    void fetch("/api/v1/library", { cache: "no-store" }).then(async (response) => {
      if (response.status === 401) return endExpiredSession();
      const { items } = (await response.json()) as { items: { title: string }[] };
      (window as { viewerCache?: unknown }).viewerCache = items;
      setTitles(items.map((item) => item.title));
    });
  }, [attempt]);
  return (
    <main>
      <h1>Library</h1>
      <ul aria-label="Saved shows">
        {titles.map((title) => (
          <li key={title}>{title}</li>
        ))}
      </ul>
      <button type="button" onClick={() => setAttempt((count) => count + 1)}>
        Refresh
      </button>
    </main>
  );
}
