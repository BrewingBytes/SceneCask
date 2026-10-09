/** Navigation target only: show detail is another issue's route. */
export default async function FixtureShow({ params }: { params: Promise<{ showId: string }> }) {
  const { showId } = await params;
  return <h1>Show {showId}</h1>;
}
