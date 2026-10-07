import type { ReactNode } from "react";
import { BrandLink } from "./brand-link";
import { Navigation, type NavigationProps } from "./navigation";
import { NotificationLink } from "./notification-link";
export interface SiteHeaderProps extends NavigationProps {
  unreadCount?: number;
  accountAction?: ReactNode;
}
export function SiteHeader({
  currentPath,
  betaEnabled,
  unreadCount,
  accountAction,
}: SiteHeaderProps) {
  return (
    <header className="sc-header">
      <div className="sc-header-inner">
        <BrandLink />
        <Navigation currentPath={currentPath} betaEnabled={betaEnabled} />
        <div className="sc-header-actions">
          {betaEnabled && <NotificationLink unreadCount={unreadCount} />}
          {accountAction}
        </div>
      </div>
    </header>
  );
}
