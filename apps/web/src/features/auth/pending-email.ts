/**
 * The address just used to register or sign in, so the verify screen can name it and resend
 * without retyping. Held in module memory only, never in storage or the URL: a reload or a new
 * tab forgets it and the screen asks for the address instead.
 */
export interface PendingEmail {
  email: string;
  /** "signin": a correct password for an unverified account (C04 403), so no new email was sent. */
  via: "signup" | "signin";
}

let pending: PendingEmail | undefined;

export function rememberPendingEmail(value: PendingEmail) {
  pending = value;
}

export function pendingEmail() {
  return pending;
}
