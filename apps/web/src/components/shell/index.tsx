import type { ReactNode } from "react";
import "../../styles/foundation.css";

// Lucide outline paths (ISC). Decorative icons always accompany visible text.
const paths = {
  Home: "m3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z",
  Discover: "m16.24 7.76-2.12 6.36-6.36 2.12 2.12-6.36 6.36-2.12Z",
  Library: "M4 6v14M8 4v16M12 8v12m4-16 4 16",
  Friends:
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2m20 0v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75",
};
function NavIcon({ name }: { name: keyof typeof paths }) {
  return (
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
      <path d={paths[name]} />
      {name === "Discover" && <circle cx="12" cy="12" r="10" />}
      {name === "Friends" && <circle cx="9" cy="7" r="4" />}
    </svg>
  );
}
export interface AppShellProps {
  children: ReactNode;
  /** Supplied by the integration router; no account data or feature flag inference. */
  currentPath: string;
  betaEnabled?: boolean;
  unreadCount?: number;
  accountAction?: ReactNode;
}
export function AppShell({
  children,
  currentPath,
  betaEnabled = false,
  unreadCount = 0,
  accountAction,
}: AppShellProps) {
  const tabs: readonly { label: keyof typeof paths; href: string }[] = [
    { label: "Home", href: "/home" },
    { label: "Discover", href: "/discover" },
    { label: "Library", href: "/library" },
    ...(betaEnabled ? [{ label: "Friends" as const, href: "/friends" }] : []),
  ] as const;
  const count = Number.isFinite(unreadCount)
    ? Math.max(0, Math.floor(unreadCount))
    : 0;
  return (
    <div className="sc-foundation">
      <a className="sc-skip" href="#sc-main">
        Skip to content
      </a>
      <header className="sc-header">
        <div className="sc-header-inner">
          <a className="sc-logo" href="/home" aria-label="SceneCask home">
            SceneCask
            <span
              aria-hidden="true"
              style={{ color: "var(--sc-color-accent)" }}
            >
              .
            </span>
          </a>
          <nav className="sc-nav" aria-label="Main navigation">
            {tabs.map((tab) => (
              <a
                key={tab.href}
                href={tab.href}
                aria-current={
                  currentPath === tab.href ||
                  currentPath.startsWith(`${tab.href}/`)
                    ? "page"
                    : undefined
                }
              >
                <NavIcon name={tab.label} />
                <span>{tab.label}</span>
              </a>
            ))}
          </nav>
          <div className="sc-header-actions">
            {betaEnabled && (
              <a
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
                  <span aria-hidden="true" className="sc-badge">
                    {count > 99 ? "99+" : count}
                  </span>
                )}
              </a>
            )}
            {accountAction}
          </div>
        </div>
      </header>
      <main id="sc-main" className="sc-main" tabIndex={-1}>
        {children}
      </main>
    </div>
  );
}
