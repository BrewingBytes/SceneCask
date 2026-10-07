"use client";
import type { TextareaHTMLAttributes } from "react";
import { Field, type FieldInfo } from "./field";
import { classes } from "./classes";
import "../../../styles/foundation.css";

export type TextAreaProps = TextareaHTMLAttributes<HTMLTextAreaElement> &
  FieldInfo;
export function TextArea({
  label,
  hint,
  error,
  id,
  className,
  "aria-describedby": describedBy,
  ...props
}: TextAreaProps) {
  return (
    <Field
      label={label}
      hint={hint}
      error={error}
      id={id}
      describedBy={describedBy}
    >
      {(control) => (
        <textarea
          {...props}
          {...control}
          className={classes("sc-input", className)}
          aria-invalid={error ? true : props["aria-invalid"]}
        />
      )}
    </Field>
  );
}
