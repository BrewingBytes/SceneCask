/**
 * Cross-tab sign-out. A tab that signs out tells the others, which leave their private screens
 * with a full document load instead of keeping the previous viewer's data on screen. One channel
 * per tab: a BroadcastChannel never delivers to the instance that posted, so the tab signing
 * out does not hear itself.
 */
let channel: BroadcastChannel | null | undefined;

function shared() {
  if (channel === undefined) {
    try {
      channel = new BroadcastChannel("scenecask-session");
    } catch {
      // No BroadcastChannel: other tabs still end on their next 401.
      channel = null;
    }
  }
  return channel;
}

export function announceSignedOut() {
  shared()?.postMessage("signed-out");
}

export function onSignedOut(listener: () => void) {
  const current = shared();
  if (!current) return () => {};
  const handle = (event: MessageEvent) => {
    if (event.data === "signed-out") listener();
  };
  current.addEventListener("message", handle);
  return () => current.removeEventListener("message", handle);
}
