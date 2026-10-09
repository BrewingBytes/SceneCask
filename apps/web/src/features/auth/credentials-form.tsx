"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useId, useState, type FormEvent } from "react";
import { Button } from "../../components/ui/basic";
import { createAuthApi, type AuthFailure } from "./api";
import { AuthFrame, FormAlert, OrDivider } from "./auth-frame";
import { failureCopy, googleProblem } from "./copy";
import { EmailField, PasswordField } from "./fields";
import { GoogleButton } from "./google-button";
import { rememberPendingEmail } from "./pending-email";
import { enterSession, HOME, signinHref } from "./routes";
import { LEAVING, useAuthForm } from "./use-auth-form";
import { emailError, newPasswordError, PASSWORD_REJECTED } from "./validation";

export interface CredentialsFormProps {
  mode: "signin" | "signup";
  /** Validated application path to enter after sign-in. */
  returnTo?: string;
  /** Fixed `?error=` code from the Google callback (C04). */
  googleError?: string;
  /** Arrived here because a private screen got 401. */
  expired?: boolean;
}

type Problem = { kind: "google"; code: string } | { kind: "credentials" } | { kind: "failure"; failure: AuthFailure };

const COPY = {
  signin: {
    title: "Sign in",
    lede: "Welcome back. Your progress is saved to your account.",
    submit: "Sign in",
  },
  signup: {
    title: "Create your account",
    lede: "Your library and progress are saved to your account, so they’re there on any device.",
    submit: "Create account",
  },
};

/** D02/D03 /auth/signin and /auth/signup: Google above email and password, separated by "or". */
export function CredentialsForm({ mode, returnTo, googleError, expired = false }: CredentialsFormProps) {
  const [api] = useState(createAuthApi);
  const router = useRouter();
  const emailId = useId();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const linkRequired = googleError === "link_required";
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<
    "email" | "password",
    Problem
  >(googleError && !linkRequired ? { kind: "google", code: googleError } : null);
  const copy = COPY[mode];

  // The callback's error code has been read; drop it so a reload does not repeat the message.
  useEffect(() => {
    const url = new URL(window.location.href);
    if (!url.searchParams.has("error")) return;
    url.searchParams.delete("error");
    window.history.replaceState(window.history.state, "", url);
  }, []);

  function rejected(failure: AuthFailure) {
    if (failure.kind === "credentials") return setProblem({ kind: "credentials" });
    const shown =
      failure.kind === "invalid" &&
      showErrors({
        email: failure.fields.includes("email") ? "Enter a valid email address." : undefined,
        password: !failure.fields.includes("password")
          ? undefined
          : mode === "signup"
            ? PASSWORD_REJECTED
            : "Enter your password.",
      });
    if (!shown) setProblem({ kind: "failure", failure });
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setProblem(null);
    const invalid = showErrors({
      email: emailError(email),
      password: mode === "signup" ? newPasswordError(password) : password ? undefined : "Enter your password.",
    });
    if (invalid) return;
    const address = email.trim();
    await run(async () => {
      if (mode === "signup") {
        const result = await api.register(address, password);
        if (!result.ok) return rejected(result.failure);
        rememberPendingEmail({ email: address, via: "signup" });
        router.push("/auth/verify");
        return LEAVING;
      }
      const result = await api.login(address, password);
      if (result.ok) {
        enterSession(returnTo ?? HOME);
        return LEAVING;
      }
      if (result.failure.kind !== "unverified") return rejected(result.failure);
      rememberPendingEmail({ email: address, via: "signin" });
      router.push("/auth/verify");
      return LEAVING;
    });
  }

  const google = problem?.kind === "google" ? googleProblem(problem.code) : null;

  return (
    <AuthFrame
      title={linkRequired ? "Sign in to link Google" : copy.title}
      lede={
        linkRequired
          ? "A SceneCask account already uses that Google email. Sign in the way you did before, then link Google from Settings. Accounts are never merged automatically."
          : copy.lede
      }
    >
      {expired && (
        <p className="sc-auth-notice" role="status">
          Your session ended. Sign in again to continue.
        </p>
      )}
      {google && (
        <FormAlert
          title={google.title}
          actions={
            <>
              <GoogleButton returnTo={returnTo} label="Try Google again" />
              <Button variant="quiet" onClick={() => document.getElementById(emailId)?.focus()}>
                Use email instead
              </Button>
            </>
          }
        >
          {google.body}
        </FormAlert>
      )}
      {!linkRequired && !google && (
        <>
          <GoogleButton returnTo={returnTo} />
          <OrDivider />
        </>
      )}
      <form ref={form} className="sc-auth-form" noValidate onSubmit={submit} aria-label={copy.title}>
        {problem?.kind === "credentials" && (
          <FormAlert
            actions={
              <Link className="sc-button sc-button-secondary" href="/auth/reset">
                Reset password
              </Link>
            }
          >
            That email and password don’t match. Try again, or reset your password.
          </FormAlert>
        )}
        {problem?.kind === "failure" && <FormAlert>{failureCopy(problem.failure)}</FormAlert>}
        <EmailField
          id={emailId}
          autoComplete={mode === "signup" ? "email" : "username"}
          value={email}
          error={errors.email}
          onValue={(value) => {
            setEmail(value);
            clearError("email");
          }}
        />
        <PasswordField
          purpose={mode === "signup" ? "new" : "current"}
          value={password}
          error={errors.password}
          onValue={(value) => {
            setPassword(value);
            clearError("password");
          }}
        />
        <Button type="submit" busy={busy}>
          {copy.submit}
        </Button>
        {mode === "signin" && (
          <Link className="sc-auth-link" href="/auth/reset">
            Forgot password?
          </Link>
        )}
      </form>
      <p className="sc-auth-switch">
        {mode === "signin" ? "New to SceneCask?" : "Already have an account?"}{" "}
        <Link className="sc-auth-link" href={mode === "signin" ? "/auth/signup" : signinHref({ returnTo })}>
          {mode === "signin" ? "Create account" : "Sign in"}
        </Link>
      </p>
    </AuthFrame>
  );
}
