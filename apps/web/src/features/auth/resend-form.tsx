"use client";
import { useState, type FormEvent } from "react";
import { Button } from "../../components/ui/basic";
import type { AuthResult } from "./api";
import { FormAlert } from "./auth-frame";
import { failureCopy } from "./copy";
import { EmailField } from "./fields";
import { useAuthForm } from "./use-auth-form";
import { RESEND_COOLDOWN_SECONDS, useCooldown } from "./use-cooldown";
import { emailError } from "./validation";

export interface ResendFormProps {
  /** The address in memory from the previous step; without it the form asks for one. */
  email?: string;
  send: (email: string) => Promise<AuthResult<unknown>>;
  label: string;
  /**
   * Seconds to wait before the first send. The API silently skips a resend within its cooldown
   * (C04), so a screen shown right after an email was queued starts cooling.
   */
  initialCooldown?: number;
}

/**
 * Sends another verification or reset email. The confirmation is the same whether or not the
 * address has an account (C04), and the button waits out the 60-second cooldown or Retry-After.
 */
export function ResendForm({ email: known, send, label, initialCooldown }: ResendFormProps) {
  const [email, setEmail] = useState(known ?? "");
  const [sent, setSent] = useState(false);
  const [cooldown, startCooldown] = useCooldown(initialCooldown);
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<"email">();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (cooldown > 0) return;
    setProblem(null);
    if (showErrors({ email: known ? undefined : emailError(email) })) return;
    await run(async () => {
      const result = await send(email.trim());
      if (result.ok) {
        setSent(true);
        startCooldown(RESEND_COOLDOWN_SECONDS);
        return;
      }
      if (result.failure.kind === "rate_limited") startCooldown(result.failure.retryAfter ?? RESEND_COOLDOWN_SECONDS);
      const shown = result.failure.kind === "invalid" && !known && showErrors({ email: "Enter a valid email address." });
      if (!shown) setProblem(result.failure);
    });
  }

  return (
    <form ref={form} className="sc-auth-form" noValidate onSubmit={submit} aria-label={label}>
      {!known && (
        <EmailField
          value={email}
          error={errors.email}
          onValue={(value) => {
            setEmail(value);
            clearError("email");
          }}
        />
      )}
      <Button type="submit" variant="secondary" busy={busy} aria-disabled={cooldown > 0 || undefined}>
        {cooldown > 0 ? `Send again in ${cooldown}s` : label}
      </Button>
      <p className="sc-auth-status" role="status">
        {sent ? "If that address can receive one, a new link is on its way." : ""}
      </p>
      {problem && <FormAlert>{failureCopy(problem)}</FormAlert>}
    </form>
  );
}
