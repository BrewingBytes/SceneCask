import type { ReactNode } from "react";

/** Bare document for the R10 account screens; each screen brings its own frame. */
export default function AuthFixtureLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body style={{ margin: 0 }}>{children}</body>
    </html>
  );
}
