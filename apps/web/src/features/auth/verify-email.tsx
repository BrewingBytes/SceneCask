"use client";
import Link from "next/link";
import { useCallback, useRef, useState } from "react";
import { Button } from "../../components/ui/basic";
import { createAuthApi, type AuthFailure } from "./api";
import { AuthFrame, FormAlert } from "./auth-frame";
import { failureCopy } from "./copy";
import { useFragmentToken } from "./fragment-token";
import { pendingEmail, type PendingEmail } from "./pending-email";
import { ResendForm } from "./resend-form";
import { AFTER_ONBOARDING, ONBOARDING } from "./routes";

type Phase =
  | { name: "start" }
  | { name: "checking" }
  | { name: "inbox"; pending?: PendingEmail }
  | { name: "expired"; pending?: PendingEmail }
  | { name: "failed"; failure: AuthFailure };

/**
 * D02 /auth/verify. Opened from the email link (`#token=`), it exchanges the token once for a
 * session and enters onboarding. Opened after sign-up, it is the "check your inbox" screen with
 * resend. There is no button that claims verification: only the real link verifies.
 */
export function VerifyEmail() {
  const [api] = useState(createAuthApi);
  const [phase, setPhase] = useState<Phase>({ name: "start" });
  // In memory only, for Retry after a network failure; the address bar was already scrubbed.
  const token = useRef<string | null>(null);
  const exchange = useCallback(async () => {
    const result = await api.verify(token.current!);
    if (result.ok) {
      // A full load replaces the token page: nothing from before verification carries over.
      window.location.replace(result.data.user.handle ? AFTER_ONBOARDING : ONBOARDING);
      return;
    }
    const { kind } = result.failure;
    // Malformed, expired and replayed tokens all need a new link.
    if (kind === "expired" || kind === "invalid") setPhase({ name: "expired", pending: pendingEmail() });
    else setPhase({ name: "failed", failure: result.failure });
  }, [api]);

  useFragmentToken((value) => {
    token.current = value;
    if (!value) return setPhase({ name: "inbox", pending: pendingEmail() });
    setPhase({ name: "checking" });
    void exchange();
  });

  const resend = (email: string) => api.resendVerification(email);

  switch (phase.name) {
    case "start":
    case "checking":
      return (
        <AuthFrame title="Verifying your email">
          <p className="sc-auth-status" role="status">
            {phase.name === "checking" ? "Checking your link…" : ""}
          </p>
        </AuthFrame>
      );
    case "failed":
      return (
        <AuthFrame title="Verifying your email">
          <FormAlert
            title="We couldn’t check your link"
            actions={
              <Button
                variant="secondary"
                onClick={() => {
                  setPhase({ name: "checking" });
                  void exchange();
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
    case "expired":
      return (
        <AuthFrame
          title="This link has expired"
          lede="Verification links work once, for 24 hours. Send yourself a new one. If you already verified, sign in."
        >
          <ResendForm email={phase.pending?.email} send={resend} label="Send a new link" />
          <Links />
        </AuthFrame>
      );
    case "inbox": {
      const { pending } = phase;
      return (
        <AuthFrame
          title={pending?.via === "signin" ? "Verify your email to sign in" : "Check your inbox"}
          lede={
            pending ? (
              pending.via === "signin" ? (
                <>
                  Your account isn’t verified yet. Open the link we emailed to <strong>{pending.email}</strong>, or
                  send a new one.
                </>
              ) : (
                <>
                  We sent a verification link to <strong>{pending.email}</strong>. Open it to finish creating your
                  account. It works for 24 hours.
                </>
              )
            ) : (
              "Open the verification link we emailed you. It works for 24 hours."
            )
          }
        >
          <ResendForm email={pending?.email} send={resend} label={pending ? "Resend link" : "Send a new link"} />
          <Links />
        </AuthFrame>
      );
    }
  }
}

function Links() {
  return (
    <p className="sc-auth-switch">
      <Link className="sc-auth-link" href="/auth/signup">
        Use a different email
      </Link>
      <Link className="sc-auth-link" href="/auth/signin">
        Back to sign in
      </Link>
    </p>
  );
}
