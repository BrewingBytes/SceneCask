import type { ReactNode } from "react";
import "../../../styles";

export interface BadgeProps {
  children: ReactNode;
  tone?: "neutral" | "accent" | "error";
}
export function Badge({ children, tone = "neutral" }: BadgeProps) {
  return <span className={`sc-badge sc-badge-${tone}`}>{children}</span>;
}
