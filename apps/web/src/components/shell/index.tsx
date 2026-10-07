import type { ReactNode } from "react";
import { SiteHeader, type SiteHeaderProps } from "./site-header";
import { PageContainer } from "./page-container";
import "../../styles";
export * from "./site-header";
export * from "./navigation";
export * from "./brand-link";
export * from "./notification-link";
export * from "./page-container";
export * from "./nav-icon";
export interface AppShellProps extends SiteHeaderProps {
  children: ReactNode;
}
export function AppShell({ children, ...header }: AppShellProps) {
  return (
    <div className="sc-foundation">
      <a className="sc-skip" href="#sc-main">
        Skip to content
      </a>
      <SiteHeader {...header} />
      <PageContainer>{children}</PageContainer>
    </div>
  );
}
