import "../../../styles";

export interface AvatarProps {
  initials: string;
  label: string;
}
const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

export function Avatar({ initials, label }: AvatarProps) {
  return (
    <span className="sc-avatar" role="img" aria-label={label}>
      {Array.from(graphemes.segment(initials), (part) => part.segment)
        .slice(0, 3)
        .join("")}
    </span>
  );
}
