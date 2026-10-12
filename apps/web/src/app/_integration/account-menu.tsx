"use client";
import { useEffect, useState } from "react";
import { createSceneCaskClient } from "@scenecask/api-client";
import { Avatar } from "../../components/ui/basic";
import { Menu, ToastViewport, useToastQueue } from "../../components/ui/overlays";
import { endExpiredSession } from "../../features/auth";
import { announceSignedOut, onSignedOut } from "./session-channel";

type Account = { displayName: string | null } | null;

function initialsOf(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  return (words.length > 1 ? words[0][0] + words[words.length - 1][0] : (words[0] ?? "?").slice(0, 2)).toUpperCase();
}

/**
 * The header account slot: the signed-in name, About and Sign out. Signing out (here or in
 * another tab) ends with a full document load, so no viewer data survives in memory or in the
 * router cache.
 */
export function AccountMenu() {
  const [open, setOpen] = useState(false);
  const [account, setAccount] = useState<Account>(null);
  const [signingOut, setSigningOut] = useState(false);
  const { toasts, show, dismiss } = useToastQueue();

  useEffect(() => {
    let current = true;
    void createSceneCaskClient()
      .GET("/session")
      .then(({ data }) => {
        if (current && data?.user) setAccount({ displayName: data.user.displayName ?? null });
      })
      .catch(() => {});
    const stop = onSignedOut(() => endExpiredSession());
    return () => {
      current = false;
      stop();
    };
  }, []);

  async function signOut() {
    setSigningOut(true);
    let csrfToken: string | undefined;
    const client = createSceneCaskClient({ getCsrfToken: () => csrfToken });
    try {
      csrfToken = (await client.GET("/session")).data?.csrfToken;
      // Already signed out (no session) is also a success (C04).
      const { response } = await client.POST("/auth/logout", { body: {} });
      if (!response.ok && response.status !== 401) throw new Error("logout failed");
    } catch {
      // The session may still be valid, so stay here rather than look signed out.
      setSigningOut(false);
      show({ id: "signout", tone: "error", message: "Couldn’t sign you out. Try again.", onRetry: () => void signOut() });
      return;
    }
    dismiss("signout");
    announceSignedOut();
    window.location.replace("/auth/signin");
  }

  const name = account?.displayName || "Your account";
  return (
    <>
      <Menu
        open={open}
        onOpenChange={setOpen}
        label="Account"
        trigger={<Avatar initials={account?.displayName ? initialsOf(account.displayName) : "?"} label={name} />}
        header={<span className="sc-account-name">{name}</span>}
        items={[
          { id: "about", label: "About SceneCask", href: "/about" },
          { id: "signout", label: signingOut ? "Signing out…" : "Sign out", disabled: signingOut, onSelect: () => void signOut() },
        ]}
      />
      <ToastViewport toasts={toasts} onDismiss={dismiss} label="Account notifications" />
    </>
  );
}
