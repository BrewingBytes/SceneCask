import type { ReactNode } from "react";
import "../../../styles";

export interface EmptyStateProps {
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}
export function EmptyState({ title, children, action }: EmptyStateProps) {
  return (
    <div className="sc-empty">
      <h3>{title}</h3>
      {children && <div className="sc-state-copy">{children}</div>}
      {action}
    </div>
  );
}
