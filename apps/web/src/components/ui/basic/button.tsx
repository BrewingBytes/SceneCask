"use client";
import type { ButtonHTMLAttributes } from "react";
import { classes } from "./classes";
import "../../../styles";

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
  const blocked =
    busy ||
    props["aria-disabled"] === true ||
    props["aria-disabled"] === "true";
  return (
    <button
      {...props}
      type={type}
      className={classes("sc-button", `sc-button-${variant}`, className)}
      disabled={disabled}
      aria-disabled={busy || disabled || props["aria-disabled"] || undefined}
      // Block the default action in the capture phase, before any listener can stop
      // propagation, and skip only this button's handlers so ancestors still see the click.
      onClickCapture={(event) => {
        if (blocked) event.preventDefault();
        else onClickCapture?.(event);
      }}
      onClick={(event) => {
        if (!blocked) onClick?.(event);
      }}
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
