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
      <svg
        width="24"
        height="24"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9m-11 12a2 2 0 0 0 4 0" />
      </svg>
      {count > 0 && (
        <span aria-hidden="true">
          <Badge>{count > 99 ? "99+" : count}</Badge>
        </span>
      )}
    </Link>
  );
}
