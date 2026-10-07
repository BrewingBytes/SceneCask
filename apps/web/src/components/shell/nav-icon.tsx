// Lucide outline paths (ISC). Decorative icons accompany visible labels.
export type NavIconName =
  | "Home"
  | "Discover"
  | "Library"
  | "Friends"
  | "Notifications";
const paths: Record<NavIconName, string> = {
  Notifications:
    "M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9m-11 12a2 2 0 0 0 4 0",
  Home: "m3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z",
  Discover: "m16.24 7.76-2.12 6.36-6.36 2.12 2.12-6.36 6.36-2.12Z",
  Library: "M4 6v14M8 4v16M12 8v12m4-16 4 16",
  Friends:
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2m20 0v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75",
};
export function NavIcon({ name }: { name: NavIconName }) {
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
