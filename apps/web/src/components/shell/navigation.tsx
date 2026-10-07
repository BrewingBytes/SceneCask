import Link from "next/link";
import { NavIcon, type NavIconName } from "./nav-icon";
import "../../styles/foundation.css";
export interface NavigationProps {
  currentPath: string;
  betaEnabled?: boolean;
}
export function Navigation({
  currentPath,
  betaEnabled = false,
}: NavigationProps) {
  const tabs: readonly { label: NavIconName; href: string }[] = [
    { label: "Home", href: "/home" },
    { label: "Discover", href: "/discover" },
    { label: "Library", href: "/library" },
    ...(betaEnabled ? [{ label: "Friends" as const, href: "/friends" }] : []),
  ];
  return (
    <nav className="sc-nav" aria-label="Main navigation">
      {tabs.map((tab) => (
        <Link
          key={tab.href}
          href={tab.href}
          aria-current={
            currentPath === tab.href || currentPath.startsWith(`${tab.href}/`)
              ? "page"
              : undefined
          }
        >
          <NavIcon name={tab.label} />
          <span>{tab.label}</span>
        </Link>
      ))}
    </nav>
  );
}
