"use client";

import "../../../styles/foundation.css";

export interface ProgressProps {
  value: number;
  max?: number;
  label: string;
}
export function Progress({ value, max = 100, label }: ProgressProps) {
  const total = Number.isFinite(max) && max > 0 ? max : 100;
  const current = Number.isFinite(value)
    ? Math.max(0, Math.min(total, value))
    : 0;
  return (
    <div
      className="sc-progress"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={current}
    >
      <span style={{ width: `${(current / total) * 100}%` }} />
    </div>
  );
}
