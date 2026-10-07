import type { CSSProperties } from "react";
import "../../../styles";

/**
 * Give one Skeleton per loading area a `label` to announce it; mark the rest
 * `decorative` so screen readers hear a single status.
 */
export type SkeletonProps = {
  width?: CSSProperties["width"];
  height?: CSSProperties["height"];
} & (
  { label: string; decorative?: never } | { decorative: true; label?: never }
);
export function Skeleton({
  label,
  width = "100%",
  height = "1em",
}: SkeletonProps) {
  return (
    <span
      className="sc-skeleton"
      role={label ? "status" : undefined}
      aria-hidden={label ? undefined : true}
      style={{ width, height }}
    >
      {label && <span className="sc-sr-only">{label}</span>}
    </span>
  );
}
