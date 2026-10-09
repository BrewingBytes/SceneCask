/** Client checks mirror C04 so most mistakes are caught before a request; the API stays authoritative. */

const characters = (text: string) => Array.from(text).length;

export const PASSWORD_MIN = 12;
export const PASSWORD_MAX = 128;
export const NAME_MAX = 80;

/** Deliberately loose: the API normalizes and decides; this only catches obvious typos. */
export function emailError(email: string) {
  return /^[^\s@]+@[^\s@]+$/.test(email.trim()) ? undefined : "Enter a valid email address.";
}

/** C04: 12–128 characters, never trimmed, no composition rules. */
export function newPasswordError(password: string) {
  const length = characters(password);
  if (length === 0) return "Enter a password.";
  if (length < PASSWORD_MIN) return `Use at least ${PASSWORD_MIN} characters.`;
  return length > PASSWORD_MAX ? `Use at most ${PASSWORD_MAX} characters.` : undefined;
}
/** Shown as the hint under new-password fields. */
export const PASSWORD_RULE = `${PASSWORD_MIN}–${PASSWORD_MAX} characters. Spaces count.`;
/** When the API rejects a password the client accepted. */
export const PASSWORD_REJECTED = `Use ${PASSWORD_MIN}–${PASSWORD_MAX} characters.`;

export function displayNameError(name: string) {
  const length = characters(name.trim());
  if (length === 0) return "Enter the name people will see.";
  return length > NAME_MAX ? `Use up to ${NAME_MAX} characters.` : undefined;
}

export const HANDLE_RULE = "3–30 lowercase letters, numbers or underscores.";
export function handleError(handle: string) {
  if (handle.length === 0) return "Choose a handle.";
  return /^[a-z0-9_]{3,30}$/.test(handle) ? undefined : `Use ${HANDLE_RULE}`;
}

/** Up to two initials for the avatar preview; initials avatars only (D01). */
export function initialsOf(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  return words
    .slice(0, 2)
    .map((word) => Array.from(word)[0].toLocaleUpperCase())
    .join("");
}
