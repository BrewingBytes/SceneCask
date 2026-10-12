import { NextResponse, type NextRequest } from "next/server";

/** Session cookie names: `__Host-` with Secure in production, plain on the localhost exception. */
const SESSION_COOKIES = ["__Host-scenecask", "scenecask"];

/**
 * Optimistic guard for private screens: without a session cookie, go to sign-in with a return
 * path before rendering anything. The API still authorizes every read; a stale cookie is caught
 * by the screens' 401 handling.
 */
export function proxy(request: NextRequest) {
  if (SESSION_COOKIES.some((name) => request.cookies.has(name))) return NextResponse.next();
  const signin = new URL("/auth/signin", request.url);
  signin.searchParams.set("returnTo", request.nextUrl.pathname + request.nextUrl.search);
  return NextResponse.redirect(signin);
}

export const config = {
  matcher: ["/home", "/discover", "/library/:path*", "/shows/:path*", "/episodes/:path*", "/onboarding/:path*"],
};
