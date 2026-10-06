"use client";
import { useState } from "react";
import { Button, Poster } from "../../../index";
export default function ArtworkFixture() {
  const [source, setSource] = useState<string | null>(null);
  return (
    <main className="sc-foundation" style={{ width: 240, padding: 20 }}>
      <Poster src={source} alt="Fixture artwork" />
      <Button
        onClick={() =>
          setSource(
            'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="200" height="300"%3E%3Crect width="200" height="300" fill="tan"/%3E%3C/svg%3E',
          )
        }
      >
        Load artwork
      </Button>
      <Button onClick={() => setSource("data:image/png;base64,invalid")}>
        Fail artwork
      </Button>
    </main>
  );
}
