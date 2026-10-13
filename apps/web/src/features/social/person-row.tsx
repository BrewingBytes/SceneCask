import Link from "next/link";
import type { ReactNode } from "react";
import { Avatar, Badge } from "../../components/ui/basic";
import type { Person } from "./api";
import { relationshipText, visibilityLabel } from "./copy";

export interface PersonRowProps {
  person: Person;
  /** Replaces the relationship line, for example "wants to follow you". */
  detail?: ReactNode;
  action?: ReactNode;
}

/** D08 person row: initials, name linking to the profile, @handle, visibility and relationship. */
export function PersonRow({ person, detail, action }: PersonRowProps) {
  const line = detail ?? relationshipText(person);
  return (
    <div className="sc-social-person">
      {/* The name beside it is the accessible text; the avatar only repeats it. */}
      <span aria-hidden="true">
        <Avatar initials={person.initials || "?"} label={person.displayName} />
      </span>
      <div className="sc-social-person-main">
        <span className="sc-social-person-name">
          <Link href={`/people/${person.id}`}>{person.displayName}</Link>{" "}
          <span className="sc-social-handle">@{person.handle}</span>
        </span>
        <span className="sc-social-person-detail">
          <Badge>{visibilityLabel(person.visibility)}</Badge>
          {line && <span>{line}</span>}
        </span>
      </div>
      {action && <div className="sc-social-person-action">{action}</div>}
    </div>
  );
}
