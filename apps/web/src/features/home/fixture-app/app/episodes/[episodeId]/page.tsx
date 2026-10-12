/** Navigation target only: episode detail is another issue's route. */
export default async function FixtureEpisode({ params }: { params: Promise<{ episodeId: string }> }) {
  const { episodeId } = await params;
  return <h1>Episode {episodeId}</h1>;
}
