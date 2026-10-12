"use client";
import Link from "next/link";
import { useEffect } from "react";
import { createAuthApi } from "../auth/api";
import { AuthFrame } from "../auth/auth-frame";
import { HOME } from "../auth/routes";
import "./home.css";

const STEPS = ["Create an account so your progress is saved", "Add a show you’re watching", "Set where you are"];

/**
 * D02 Welcome at `/`: what SceneCask is, then create account or sign in. It carries no account
 * data; a browser that is already signed in goes straight to Home.
 */
export function Welcome() {
  useEffect(() => {
    let current = true;
    void createAuthApi()
      .session()
      .then((result) => {
        if (current && result.ok && result.data.user) window.location.replace(HOME);
      });
    return () => {
      current = false;
    };
  }, []);

  // The account screens' brand-only frame, in the export's wider 560px Welcome column.
  return (
    <div className="sc-welcome">
      <AuthFrame
        title="Keep track of your shows. Without spoilers."
        lede="Episode titles, stills and summaries stay hidden until you’ve watched them. SceneCask doesn’t stream video; it keeps your place."
      >
        <ol className="sc-welcome-steps">
          {STEPS.map((step, index) => (
            <li key={step}>
              <span className="sc-code" aria-hidden="true">
                {String(index + 1).padStart(2, "0")}
              </span>
              {step}
            </li>
          ))}
        </ol>
        <div className="sc-welcome-actions">
          <Link className="sc-button sc-button-primary" href="/auth/signup">
            Create account
          </Link>
          <Link className="sc-button sc-button-secondary" href="/auth/signin">
            Sign in
          </Link>
        </div>
      </AuthFrame>
    </div>
  );
}
