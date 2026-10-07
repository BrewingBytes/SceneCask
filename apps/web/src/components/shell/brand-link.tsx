import Link from "next/link";
import "../../styles/foundation.css";
export function BrandLink() {
  return (
    <Link className="sc-logo" href="/home" aria-label="SceneCask home">
      SceneCask
      <span aria-hidden="true" style={{ color: "var(--sc-color-accent)" }}>
        .
      </span>
    </Link>
  );
}
