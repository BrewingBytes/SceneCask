"use client";
import type { ButtonHTMLAttributes } from "react";
import { classes } from "./classes";
import "../../../styles/foundation.css";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "quiet" | "destructive";
  busy?: boolean;
}
export function Button({
  variant = "primary",
  busy = false,
  disabled,
  children,
  className,
  type = "button",
  onClick,
  onClickCapture,
  ...props
}: ButtonProps) {
  return (
    <button
      {...props}
      type={type}
      className={classes("sc-button", `sc-button-${variant}`, className)}
      disabled={disabled}
      aria-disabled={busy || disabled || props["aria-disabled"] || undefined}
      onClickCapture={(event) => {
        if (
          busy ||
          disabled ||
          props["aria-disabled"] === true ||
          props["aria-disabled"] === "true"
        ) {
          event.preventDefault();
          event.stopPropagation();
          return;
        }
        onClickCapture?.(event);
      }}
      onClick={onClick}
      aria-busy={busy || undefined}
    >
      {children}
      {busy && (
        <span className="sc-busy" aria-hidden="true">
          {" "}
          ···
        </span>
      )}
    </button>
  );
}
