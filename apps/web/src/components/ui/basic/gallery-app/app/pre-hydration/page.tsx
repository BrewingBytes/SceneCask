import { Poster } from "../../../poster";
export default function HydrationFixture() {
  return (
    <main className="sc-foundation" style={{ width: 240, padding: 20 }}>
      <Poster src="data:image/png;base64,invalid" alt="SSR artwork" />
    </main>
  );
}
