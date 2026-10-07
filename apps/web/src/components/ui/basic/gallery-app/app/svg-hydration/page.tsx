"use client";
import { useState } from "react";
import { Poster } from "../../../poster";
import { Button } from "../../../button";
const source =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 30"><rect width="20" height="30" fill="red"/></svg>',
  );
export default function SvgFixture() {
  const [count, setCount] = useState(0);
  return (
    <main className="sc-foundation" style={{ width: 240, padding: 20 }}>
      <Poster src={source} alt="Valid SVG" />
      <Button onClick={() => setCount(count + 1)}>Render {count}</Button>
    </main>
  );
}
