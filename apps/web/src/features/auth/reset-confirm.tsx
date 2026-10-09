"use client";
import Link from "next/link";
import { useRef, useState, type FormEvent } from "react";
import { Button } from "../../components/ui/basic";
import { createAuthApi } from "./api";
import { AuthFrame, FormAlert, OrDivider } from "./auth-frame";
import { failureCopy } from "./copy";
import { PasswordField } from "./fields";
import { useFragmentToken } from "./fragment-token";
import { GoogleButton } from "./google-button";
import { useAuthForm } from "./use-auth-form";
import { newPasswordError, PASSWORD_REJECTED } from "./validation";

type Phase = "start" | "form" | "missing" | "expired" | "done";

/**
 * D02 /auth/reset/confirm, opened from the reset email (`#token=`). A successful reset signs
 * out every session (C04), so it ends on "sign in". An expired or replayed link offers a new
 * link and the other sign-in methods.
 */
export function ResetConfirm() {
  const [api] = useState(createAuthApi);
  const token = useRef<string | null>(null);
  const [phase, setPhase] = useState<Phase>("start");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<"password" | "confirm">();

  useFragmentToken((value) => {
    token.current = value;
    setProblem(null);
    setPhase(value ? "form" : "missing");
  });

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setProblem(null);
    const invalid = showErrors({
      password: newPasswordError(password),
      confirm: !confirm ? "Enter the password again." : confirm !== password ? "The passwords don’t match." : undefined,
    });
    if (invalid) return;
    await run(async () => {
      const result = await api.resetPassword(token.current!, password);
      if (result.ok) {
        token.current = null;
        return setPhase("done");
      }
      const { failure } = result;
      if (failure.kind === "expired" || (failure.kind === "invalid" && failure.fields.includes("token")))
        return setPhase("expired");
      const shown = failure.kind === "invalid" && showErrors({ password: PASSWORD_REJECTED });
      if (!shown) setProblem(failure);
    });
  }

  switch (phase) {
    case "done":
      return (
        <AuthFrame
          title="Password changed"
          lede="You’ve been signed out everywhere. Sign in with your new password."
        >
          <Link className="sc-button sc-button-primary sc-auth-wide" href="/auth/signin">
            Sign in
          </Link>
        </AuthFrame>
      );
    case "expired":
    case "missing":
      return (
        <AuthFrame
          title={phase === "expired" ? "This reset link has expired" : "This reset link is incomplete"}
          lede={
            phase === "expired"
              ? "Reset links work once, for 30 minutes. Send yourself a new one, or sign in another way."
              : "Open the link from your email again, or send yourself a new one."
          }
        >
          <Link className="sc-button sc-button-primary sc-auth-wide" href="/auth/reset">
            Send a new link
          </Link>
          <OrDivider />
          <GoogleButton />
          <p className="sc-auth-switch">
            Remembered your password?{" "}
            <Link className="sc-auth-link" href="/auth/signin">
              Sign in
            </Link>
          </p>
        </AuthFrame>
      );
    default:
      return (
        <AuthFrame title="Choose a new password" lede="This signs you out on every device.">
          {phase === "form" && (
            <form ref={form} className="sc-auth-form" noValidate onSubmit={submit} aria-label="Choose a new password">
              {problem && <FormAlert>{failureCopy(problem)}</FormAlert>}
              <PasswordField
                label="New password"
                purpose="new"
                value={password}
                error={errors.password}
                onValue={(value) => {
                  setPassword(value);
                  clearError("password");
                }}
              />
              <PasswordField
                label="Confirm new password"
                purpose="confirm"
                value={confirm}
                error={errors.confirm}
                onValue={(value) => {
                  setConfirm(value);
                  clearError("confirm");
                }}
              />
              <Button type="submit" busy={busy}>
                Change password
              </Button>
            </form>
          )}
        </AuthFrame>
      );
  }
}
