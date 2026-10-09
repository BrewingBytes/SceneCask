"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent } from "react";
import { Avatar, Button, TextField } from "../../components/ui/basic";
import { createAuthApi, type AuthFailure } from "./api";
import { AuthFrame, FormAlert } from "./auth-frame";
import { failureCopy } from "./copy";
import { AFTER_ONBOARDING, endExpiredSession, ONBOARDING, signinHref } from "./routes";
import { LEAVING, useAuthForm } from "./use-auth-form";
import { displayNameError, HANDLE_RULE, handleError, initialsOf } from "./validation";

type Phase = { name: "loading" } | { name: "ready" } | { name: "failed"; failure: AuthFailure };

/**
 * D03 /onboarding/profile: display name and a unique, permanent handle, with an initials
 * preview. Save or skip (alpha) both continue to Discover. Accounts that already have a handle
 * skip the screen; signed-out visitors go to sign-in and come back.
 */
export function ProfileOnboarding() {
  const [api] = useState(createAuthApi);
  const router = useRouter();
  const [phase, setPhase] = useState<Phase>({ name: "loading" });
  const [displayName, setDisplayName] = useState("");
  const [handle, setHandle] = useState("");
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<
    "displayName" | "handle"
  >();

  // Each Retry is a new attempt; a superseded or unmounted attempt never settles.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    void api.session().then((result) => {
      if (!live) return;
      if (!result.ok) return setPhase({ name: "failed", failure: result.failure });
      const { user } = result.data;
      if (!user) return window.location.replace(signinHref({ returnTo: ONBOARDING }));
      if (user.handle) return window.location.replace(AFTER_ONBOARDING);
      setDisplayName(user.displayName);
      setPhase({ name: "ready" });
    });
    return () => {
      live = false;
    };
  }, [api, attempt]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setProblem(null);
    if (showErrors({ displayName: displayNameError(displayName), handle: handleError(handle) })) return;
    await run(async () => {
      const result = await api.updateProfile(displayName.trim(), handle);
      if (result.ok) {
        router.push(AFTER_ONBOARDING);
        return LEAVING;
      }
      const { failure } = result;
      if (failure.kind === "auth") {
        endExpiredSession(ONBOARDING);
        return LEAVING;
      }
      const shown =
        failure.kind === "handle_taken"
          ? showErrors({ handle: "That handle is taken. Try another." })
          : failure.kind === "invalid" &&
            showErrors({
              displayName: failure.fields.includes("displayName") ? "Use 1–80 characters." : undefined,
              handle: failure.fields.includes("handle") ? `Use ${HANDLE_RULE}` : undefined,
            });
      if (!shown) setProblem(failure);
    });
  }

  const title = "Set up your profile";
  if (phase.name === "loading")
    return (
      <AuthFrame title={title}>
        <p className="sc-auth-status" role="status">
          Loading your account…
        </p>
      </AuthFrame>
    );
  if (phase.name === "failed")
    return (
      <AuthFrame title={title}>
        <FormAlert
          title="Your account didn’t load"
          actions={
            <Button
              variant="secondary"
              onClick={() => {
                setPhase({ name: "loading" });
                setAttempt((count) => count + 1);
              }}
            >
              Retry
            </Button>
          }
        >
          {failureCopy(phase.failure)}
        </FormAlert>
      </AuthFrame>
    );

  const initials = initialsOf(displayName);
  return (
    <AuthFrame
      title={title}
      lede="This is how you’ll appear if you connect with friends later. You can skip it for now and still track every show."
    >
      <form ref={form} className="sc-auth-form" noValidate onSubmit={submit} aria-label={title}>
        <div className="sc-auth-preview">
          <Avatar initials={initials || "?"} label={initials ? `Avatar preview: ${initials}` : "Avatar preview"} />
          <p>
            <strong>{displayName.trim() || "Your name"}</strong>
            <span>@{handle || "handle"}</span>
          </p>
        </div>
        {problem && <FormAlert>{failureCopy(problem)}</FormAlert>}
        <TextField
          label="Display name"
          autoComplete="name"
          value={displayName}
          error={errors.displayName}
          onChange={(event) => {
            setDisplayName(event.target.value);
            clearError("displayName");
          }}
        />
        <TextField
          label="Handle"
          hint={`${HANDLE_RULE} You can’t change it later.`}
          autoComplete="off"
          autoCapitalize="none"
          spellCheck={false}
          value={handle}
          error={errors.handle}
          onChange={(event) => {
            setHandle(event.target.value);
            clearError("handle");
          }}
        />
        <div className="sc-auth-actions">
          <Button type="submit" busy={busy}>
            Save profile
          </Button>
          <Link className="sc-button sc-button-quiet" href={AFTER_ONBOARDING}>
            Skip for now
          </Link>
        </div>
      </form>
    </AuthFrame>
  );
}
