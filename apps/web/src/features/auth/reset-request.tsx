"use client";
import Link from "next/link";
import { useState, type FormEvent } from "react";
import { Button } from "../../components/ui/basic";
import { createAuthApi } from "./api";
import { AuthFrame, FormAlert } from "./auth-frame";
import { failureCopy } from "./copy";
import { EmailField } from "./fields";
import { ResendForm } from "./resend-form";
import { useAuthForm } from "./use-auth-form";
import { RESEND_COOLDOWN_SECONDS } from "./use-cooldown";
import { emailError } from "./validation";

/**
 * D02 /auth/reset: request a reset link, then "check your inbox". The confirmation never says
 * whether the address has an account (C04). Google-only accounts set a password this way.
 */
export function ResetRequest() {
  const [api] = useState(createAuthApi);
  const [email, setEmail] = useState("");
  const [sentTo, setSentTo] = useState<string | null>(null);
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<"email">();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setProblem(null);
    if (showErrors({ email: emailError(email) })) return;
    const address = email.trim();
    await run(async () => {
      const result = await api.requestReset(address);
      if (result.ok) return setSentTo(address);
      const shown = result.failure.kind === "invalid" && showErrors({ email: "Enter a valid email address." });
      if (!shown) setProblem(result.failure);
    });
  }

  if (sentTo)
    return (
      <AuthFrame
        title="Check your inbox"
        lede={
          <>
            If there’s an account for <strong>{sentTo}</strong>, a reset link is on its way. It works for 30 minutes.
          </>
        }
      >
        <ResendForm
          email={sentTo}
          send={(address) => api.requestReset(address)}
          label="Resend link"
          initialCooldown={RESEND_COOLDOWN_SECONDS}
        />
        <p className="sc-auth-switch">
          <Button variant="quiet" onClick={() => setSentTo(null)}>
            Use a different email
          </Button>
          <Link className="sc-auth-link" href="/auth/signin">
            Back to sign in
          </Link>
        </p>
      </AuthFrame>
    );

  return (
    <AuthFrame title="Reset your password" lede="Enter the email you signed up with and we’ll send a reset link.">
      <form ref={form} className="sc-auth-form" noValidate onSubmit={submit} aria-label="Reset your password">
        {problem && <FormAlert>{failureCopy(problem)}</FormAlert>}
        <EmailField
          value={email}
          error={errors.email}
          onValue={(value) => {
            setEmail(value);
            clearError("email");
          }}
        />
        <Button type="submit" busy={busy}>
          Send reset link
        </Button>
      </form>
      <p className="sc-auth-switch">
        Remembered it?{" "}
        <Link className="sc-auth-link" href="/auth/signin">
          Back to sign in
        </Link>
      </p>
    </AuthFrame>
  );
}
