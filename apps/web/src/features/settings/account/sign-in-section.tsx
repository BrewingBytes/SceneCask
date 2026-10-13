"use client";
import { useState } from "react";
import { Badge, Button } from "../../../components/ui/basic";
import { ConfirmDialog } from "../../../components/ui/overlays";
import { announceSignedOut } from "../../../app/_integration/session-channel";
import { endExpiredSession, googleProblem, googleStartHref, ReauthDialog } from "../../auth";
import type { Failure, Identities, SettingsApi, User } from "./api";
import { googleReturnTo, LAST_METHOD_EXPLANATION, settingsFailureCopy, type GoogleReturn } from "./copy";
import { SectionStatus, SettingsRow, SettingsSection } from "./section";

export interface SignInSectionProps {
  api: SettingsApi;
  user: User;
  identities: Identities;
  onIdentities: (identities: Identities) => void;
  /** A Google flow this page started has returned (`?google=`, plus `?error=` on failure). */
  returned: { action: GoogleReturn; error: string | null } | null;
}

type Pending = "link" | "unlink";

/**
 * D04 Sign-in methods. Linking and unlinking Google need a sign-in confirmed within 5 minutes
 * (C04): the reauth dialog runs first and then resumes the pending action. Unlinking the last
 * method is disabled with an explanation; the API's 409 is handled the same way.
 */
export function SignInSection({ api, user, identities, onIdentities, returned }: SignInSectionProps) {
  const { passwordEnabled, googleLinked } = identities;
  const [confirmUnlink, setConfirmUnlink] = useState(returned?.action === "unlink" && !returned.error);
  const [reauthFor, setReauthFor] = useState<Pending | null>(null);
  const [busy, setBusy] = useState<"unlink" | "link" | "password" | "signout" | null>(null);
  const [dialogError, setDialogError] = useState("");
  // Returning from linking: the identities just read decide whether it worked, not the URL.
  const [status, setStatus] = useState(() =>
    returned?.action === "link" && !returned.error && googleLinked ? "Google is linked. You can now sign in with Google." : "",
  );
  const [problem, setProblem] = useState(() => {
    if (!returned?.error) return "";
    const copy = googleProblem(returned.error);
    return `${copy.title}. ${copy.body}`;
  });

  async function refresh() {
    const result = await api.identities();
    if (result.ok) onIdentities(result.data);
  }

  function fail(failure: Failure, show: (copy: string) => void) {
    if (failure.kind === "auth") return endExpiredSession();
    show(settingsFailureCopy(failure));
  }

  async function unlink() {
    setBusy("unlink");
    setDialogError("");
    const result = await api.unlinkGoogle();
    setBusy(null);
    if (result.ok) {
      setConfirmUnlink(false);
      await refresh();
      return setStatus("Google is unlinked. Sign in with your email and password.");
    }
    // A stale confirmation is 401 here: confirm again, then this action resumes.
    if (result.failure.kind === "auth") return setReauthFor("unlink");
    if (result.failure.kind === "last_method") {
      setDialogError(LAST_METHOD_EXPLANATION);
      return void refresh();
    }
    fail(result.failure, setDialogError);
  }

  function startLink() {
    setBusy("link");
    window.location.assign(googleStartHref("link", googleReturnTo("link")));
  }

  async function requestPassword() {
    setBusy("password");
    setProblem("");
    setStatus("");
    const result = await api.requestPassword(user.email);
    setBusy(null);
    if (result.ok)
      return setStatus(
        `We sent a link to ${user.email}. Open it to set a password. Setting it signs you out on every device.`,
      );
    fail(result.failure, setProblem);
  }

  async function signOut() {
    setBusy("signout");
    setProblem("");
    const result = await api.logout();
    // Already signed out (401) is also a success (C04).
    if (result.ok || result.failure.kind === "auth") {
      announceSignedOut();
      return window.location.replace("/auth/signin");
    }
    setBusy(null);
    setProblem(`Couldn’t sign you out. ${settingsFailureCopy(result.failure)}`);
  }

  const onlyGoogle = googleLinked && !passwordEnabled;
  return (
    <SettingsSection id="signin" title="Sign-in methods">
      <SettingsRow
        title="Email and password"
        detail={
          passwordEnabled ? (
            <>
              <Badge>On</Badge> {user.email}
            </>
          ) : (
            "No password yet. We’ll email you a link to set one."
          )
        }
        action={
          !passwordEnabled && (
            <Button variant="secondary" busy={busy === "password"} onClick={() => void requestPassword()}>
              Set a password
            </Button>
          )
        }
      />
      <SettingsRow
        title="Google"
        detail={
          googleLinked ? (
            <>
              <Badge>Linked</Badge> {onlyGoogle && <span id="sc-settings-last-method">{LAST_METHOD_EXPLANATION}</span>}
            </>
          ) : (
            "Not linked. Link it to sign in with Google too."
          )
        }
        action={
          googleLinked ? (
            <Button
              variant="secondary"
              aria-disabled={onlyGoogle || undefined}
              aria-describedby={onlyGoogle ? "sc-settings-last-method" : undefined}
              onClick={() => {
                if (onlyGoogle) return;
                setDialogError("");
                setConfirmUnlink(true);
              }}
            >
              Unlink Google
            </Button>
          ) : (
            <Button variant="secondary" busy={busy === "link"} onClick={() => setReauthFor("link")}>
              Link Google
            </Button>
          )
        }
      />
      <SettingsRow
        title="Sign out"
        detail="Sign out of SceneCask on this device."
        action={
          <Button variant="secondary" busy={busy === "signout"} onClick={() => void signOut()}>
            Sign out
          </Button>
        }
      />
      <SectionStatus status={status} problem={problem} />
      <ConfirmDialog
        open={confirmUnlink}
        onOpenChange={setConfirmUnlink}
        title="Unlink Google?"
        confirmLabel="Unlink Google"
        busy={busy === "unlink"}
        onConfirm={() => void unlink()}
        error={dialogError}
      >
        <p>You’ll sign in with your email and password. You can link Google again later from Settings.</p>
      </ConfirmDialog>
      <ReauthDialog
        open={reauthFor !== null}
        onOpenChange={(open) => !open && setReauthFor(null)}
        passwordEnabled={passwordEnabled}
        // Linking needs the password: Google can't confirm an identity that isn't linked yet.
        googleLinked={reauthFor === "unlink" && googleLinked}
        returnTo={googleReturnTo("unlink")}
        onReauthenticated={() => {
          const pending = reauthFor;
          setReauthFor(null);
          if (pending === "link") startLink();
          else void unlink();
        }}
      />
    </SettingsSection>
  );
}
