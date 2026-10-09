"use client";
import { useEffect, useId, useRef, type ReactNode } from "react";
import { BrandLink, PageContainer } from "../../components/shell";
import "./auth.css";

export interface AuthFrameProps {
  /** Visible heading and the section's accessible name. */
  title: string;
  lede?: ReactNode;
  children?: ReactNode;
}

/**
 * The 480px account column (D03) under a brand-only header: account screens carry no app
 * navigation. When the title changes within a screen (a sent form, an expired link), focus
 * moves to the new heading so the change is announced and keyboard position is not lost.
 */
export function AuthFrame({ title, lede, children }: AuthFrameProps) {
  const id = useId();
  const heading = useRef<HTMLHeadingElement>(null);
  const shown = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (shown.current !== undefined && shown.current !== title) heading.current?.focus();
    shown.current = title;
  }, [title]);

  return (
    <div className="sc-foundation sc-auth">
      <a className="sc-skip" href="#sc-main">
        Skip to content
      </a>
      <header className="sc-header">
        <div className="sc-header-inner">
          <BrandLink />
        </div>
      </header>
      <PageContainer>
        <section className="sc-auth-column" aria-labelledby={id}>
          <h1 id={id} ref={heading} tabIndex={-1}>
            {title}
          </h1>
          {lede && <p className="sc-auth-lede">{lede}</p>}
          {children}
        </section>
      </PageContainer>
    </div>
  );
}

export interface FormAlertProps {
  title?: string;
  children: ReactNode;
  actions?: ReactNode;
}

/** Form-level failure (D03): announced once, with recovery actions beside it. */
export function FormAlert({ title, children, actions }: FormAlertProps) {
  return (
    <div className="sc-error sc-auth-alert">
      <div role="alert">
        {title && <h2>{title}</h2>}
        <p>{children}</p>
      </div>
      {actions && <div className="sc-actions">{actions}</div>}
    </div>
  );
}

/** "or" between Google and email sign-in (D03). */
export function OrDivider() {
  return (
    <p className="sc-auth-divider">
      <span>or</span>
    </p>
  );
}
