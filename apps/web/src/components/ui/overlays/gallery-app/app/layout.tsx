import type { ReactNode } from "react";
export const metadata = { title: "SceneCask overlay gallery" };
export default function GalleryLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body style={{ margin: 0 }}>{children}</body>
    </html>
  );
}
