import type { ReactNode } from "react";
import "../../styles";
export interface PageContainerProps {
  children: ReactNode;
  id?: string;
}
export function PageContainer({
  children,
  id = "sc-main",
}: PageContainerProps) {
  return (
    <main id={id} className="sc-main" tabIndex={-1}>
      {children}
    </main>
  );
}
