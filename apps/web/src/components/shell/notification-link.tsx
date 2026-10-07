import { NavIcon } from "./nav-icon";
import Link from "next/link";
import { Badge } from "../ui/basic/badge";
export interface NotificationLinkProps {
  unreadCount?: number;
}
export function NotificationLink({ unreadCount = 0 }: NotificationLinkProps) {
  const count = Number.isFinite(unreadCount)
    ? Math.max(0, Math.floor(unreadCount))
    : 0;
  return (
    <Link
      className="sc-notifications"
      href="/notifications"
      aria-label={`Notifications${count ? `, ${count} unread` : ""}`}
    >
      <NavIcon name="Notifications" />
      {count > 0 && (
        <span aria-hidden="true">
          <Badge>{count > 99 ? "99+" : count}</Badge>
        </span>
      )}
    </Link>
  );
}
