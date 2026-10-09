"use client";
import { useId, useState, type FormEvent } from "react";
import { Button } from "../../components/ui/basic";
import { Dialog } from "../../components/ui/overlays";
import { createAuthApi, type AuthApi } from "./api";
import { OrDivider } from "./auth-frame";
import { failureCopy } from "./copy";
import { PasswordField } from "./fields";
import { GoogleButton } from "./google-button";
import { endExpiredSession } from "./routes";
import { LEAVING, useAuthForm } from "./use-auth-form";

export interface ReauthDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The session is fresh for 5 minutes (C04): resume the pending link, unlink or delete. */
  onReauthenticated: () => void;
  /** From `GET /me/identities`. */
  passwordEnabled: boolean;
  googleLinked: boolean;
  /** Application path Google returns to; the caller resumes there (it reads `?error=` on failure). */
  returnTo: string;
  /** The caller's adapter, so its CSRF token is refreshed after the session rotates. */
  api?: AuthApi;
}

/**
 * D04 reauthentication: password, or Continue with Google for accounts that have it. Busy
 * blocks dismissal so an in-flight confirmation is never silently dropped.
 */
export function ReauthDialog({
  open,
  onOpenChange,
  onReauthenticated,
  passwordEnabled,
  googleLinked,
  returnTo,
  api: shared,
}: ReauthDialogProps) {
  const [own] = useState(createAuthApi);
  const api = shared ?? own;
  const formId = useId();
  const [password, setPassword] = useState("");
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<"password">();

  function close(next: boolean) {
    if (!next) {
      setPassword("");
      clearError("password");
      setProblem(null);
    }
    onOpenChange(next);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setProblem(null);
    if (showErrors({ password: password ? undefined : "Enter your password." })) return;
    await run(async () => {
      const result = await api.reauth(password);
      if (result.ok) {
        close(false);
        return onReauthenticated();
      }
      const { failure } = result;
      if (failure.kind === "auth") {
        endExpiredSession();
        return LEAVING;
      }
      const shown =
        (failure.kind === "credentials" || failure.kind === "invalid") &&
        showErrors({ password: "That password isn’t right." });
      if (!shown) setProblem(failure);
    });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={close}
      busy={busy}
      title="Confirm it’s you"
      description="For your security, confirm your sign-in before this change. It stays confirmed for 5 minutes."
      footer={
        <>
          <Button variant="secondary" aria-disabled={busy || undefined} onClick={() => close(false)}>
            Cancel
          </Button>
          {passwordEnabled && (
            <Button type="submit" form={formId} busy={busy}>
              Confirm
            </Button>
          )}
        </>
      }
    >
      <div className="sc-auth-form">
        {passwordEnabled && (
          <form ref={form} id={formId} className="sc-auth-form" noValidate onSubmit={submit} aria-label="Confirm with your password">
            <PasswordField
              purpose="current"
              data-autofocus
              value={password}
              error={errors.password}
              onValue={(value) => {
                setPassword(value);
                clearError("password");
              }}
            />
          </form>
        )}
        {passwordEnabled && googleLinked && <OrDivider />}
        {googleLinked && <GoogleButton intent="reauth" returnTo={returnTo} />}
        {/* Mounted with the dialog so a later failure is announced when it appears. */}
        <div role="alert" className="sc-modal-error">
          {problem && failureCopy(problem)}
        </div>
      </div>
    </Dialog>
  );
}
