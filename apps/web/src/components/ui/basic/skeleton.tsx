import type { CSSProperties } from "react";
import "../../../styles";

export interface SkeletonProps {
  label?: string;
  width?: CSSProperties["width"];
  height?: CSSProperties["height"];
}
export function Skeleton({
  label = "Loading",
  width = "100%",
  height = "1em",
}: SkeletonProps) {
  return (
    <span className="sc-skeleton" role="status" style={{ width, height }}>
      <span className="sc-sr-only">{label}</span>
    </span>
  );
}
