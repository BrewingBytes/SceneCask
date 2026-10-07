import type { ReactNode } from "react";
import { Poster, type PosterProps } from "./poster";
export interface ArtworkFigureProps extends PosterProps {
  caption: ReactNode;
}
export function ArtworkFigure({ caption, ...poster }: ArtworkFigureProps) {
  return (
    <figure className="sc-art-figure">
      <Poster {...poster} />
      <figcaption>{caption}</figcaption>
    </figure>
  );
}
