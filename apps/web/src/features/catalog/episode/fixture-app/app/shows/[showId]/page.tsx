/** Navigation target only: show detail is R21's route and has its own suite. */
export default async function FixtureShow({ params }: { params: Promise<{ showId: string }> }) {
  const { showId } = await params;
  return <h1>Show {showId}</h1>;
}
