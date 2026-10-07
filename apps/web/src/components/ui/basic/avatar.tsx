"use client";

import "../../../styles/foundation.css";

export interface AvatarProps {
  initials: string;
  label: string;
}
export function Avatar({ initials, label }: AvatarProps) {
  return (
    <span className="sc-avatar" role="img" aria-label={label}>
      {Array.from(initials).slice(0, 3).join("")}
    </span>
  );
}
