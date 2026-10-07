import Link from "next/link";
import "../../styles";
export interface BrandLinkProps {
  homeHref?: "/" | "/home";
}
export function BrandLink({ homeHref = "/" }: BrandLinkProps) {
  return (
    <Link className="sc-logo" href={homeHref} aria-label="SceneCask home">
      SceneCask
      <span aria-hidden="true" style={{ color: "var(--sc-color-accent)" }}>
        .
      </span>
    </Link>
  );
}
