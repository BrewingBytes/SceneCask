"use client";
import { useCallback, useState } from "react";
import { classes } from "./classes";
import "../../../styles";

export interface PosterProps {
  src?: string | null;
  alt: string;
  aspect?: "poster" | "still";
  className?: string;
}
export function Poster({
  src,
  alt,
  aspect = "poster",
  className,
}: PosterProps) {
  const [failedSource, setFailedSource] = useState<string | null>(null);
  const inspectImage = useCallback(
    (element: HTMLImageElement | null) => {
      if (!element) return;
      let current = true;
      // Zero intrinsic width alone does not establish failure (e.g. SVG artwork).
      // Decoding distinguishes a completed failure that happened before hydration.
      if (element.complete && element.naturalWidth === 0) {
        void element.decode().catch(() => {
          if (current) setFailedSource(src ?? null);
        });
      }
      return () => {
        current = false;
      };
    },
    [src],
  );
  const hasImage = !!src && failedSource !== src;
  return (
    <div className={classes("sc-art", `sc-art-${aspect}`, className)}>
      {hasImage ? (
        // Native image keeps failure handling and reserved dimensions together; callers supply authorized artwork only.
        // eslint-disable-next-line @next/next/no-img-element
        <img
          ref={inspectImage}
          src={src}
          alt={alt}
          width={aspect === "poster" ? 200 : 320}
          height={aspect === "poster" ? 300 : 180}
          onError={() => setFailedSource(src)}
        />
      ) : (
        <div
          className="sc-art-fallback"
          role={alt ? "img" : undefined}
          aria-label={alt || undefined}
          aria-hidden={!alt || undefined}
        >
          <span aria-hidden="true">SC</span>
        </div>
      )}
    </div>
  );
}
