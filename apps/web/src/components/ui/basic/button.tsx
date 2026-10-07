"use client";
import type { ButtonHTMLAttributes } from "react";
import { classes } from "./classes";
import "../../../styles";

// Activation handlers a blocked button must not run; click is handled separately.
const activationHandlers = [
  "onPointerDown",
  "onPointerUp",
  "onMouseDown",
  "onMouseUp",
  "onKeyDown",
  "onKeyUp",
  "onTouchStart",
  "onTouchEnd",
] as const;

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
  const guarded = blocked
    ? Object.fromEntries(activationHandlers.map((name) => [name, undefined]))
    : {};
  return (
    <button
      {...props}
      {...guarded}
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
