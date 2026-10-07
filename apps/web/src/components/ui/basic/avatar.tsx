import Graphemer from "graphemer";
import "../../../styles";

export interface AvatarProps {
  initials: string;
  label: string;
}
let fallback: Graphemer | undefined;
let graphemes: Intl.Segmenter | undefined;
function avatarInitials(initials: string) {
  if (typeof Intl.Segmenter !== "function") {
    fallback ??= new Graphemer();
    return fallback.splitGraphemes(initials).slice(0, 3).join("");
  }
  graphemes ??= new Intl.Segmenter(undefined, { granularity: "grapheme" });
  return Array.from(graphemes.segment(initials), (part) => part.segment)
    .slice(0, 3)
    .join("");
}

export function Avatar({ initials, label }: AvatarProps) {
  return (
    <span className="sc-avatar" role="img" aria-label={label}>
      {avatarInitials(initials)}
    </span>
  );
}
