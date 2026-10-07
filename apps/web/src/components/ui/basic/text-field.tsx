"use client";
import type { InputHTMLAttributes } from "react";
import { Field, type FieldInfo } from "./field";
import { classes } from "./classes";
import "../../../styles/foundation.css";

export type TextFieldProps = InputHTMLAttributes<HTMLInputElement> & FieldInfo;
export function TextField({
  label,
  hint,
  error,
  id,
  className,
  "aria-describedby": describedBy,
  ...props
}: TextFieldProps) {
  return (
    <Field
      label={label}
      hint={hint}
      error={error}
      id={id}
      describedBy={describedBy}
    >
      {(control) => (
        <input
          {...props}
          {...control}
          className={classes("sc-input", className)}
          aria-invalid={error ? true : props["aria-invalid"]}
        />
      )}
    </Field>
  );
}
