"use client";
import type { ReactNode } from "react";
import "../../../styles/foundation.css";

export interface BadgeProps {
  children: ReactNode;
  tone?: "neutral" | "accent" | "error";
}
export function Badge({ children, tone = "neutral" }: BadgeProps) {
  return <span className={`sc-badge sc-badge-${tone}`}>{children}</span>;
}
