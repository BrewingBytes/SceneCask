"use client";
import { useId, type ReactNode } from "react";
import { classes } from "./classes";
import "../../../styles/foundation.css";

export interface FieldInfo {
  label: string;
  hint?: string;
  error?: string;
}
export interface FieldControlProps {
  id: string;
  "aria-describedby"?: string;
  "aria-invalid"?: true;
}
export interface FieldProps extends FieldInfo {
  id?: string;
  describedBy?: string;
  children: (props: FieldControlProps) => ReactNode;
}
/** One source for labels, generated IDs, hints, errors and accessible descriptions. */
export function Field({
  label,
  hint,
  error,
  id,
  describedBy,
  children,
}: FieldProps) {
  const generated = useId();
  const fieldId = id ?? generated;
  const description =
    classes(
      describedBy,
      hint && `${fieldId}-hint`,
      error && `${fieldId}-error`,
    ) || undefined;
  return (
    <div className="sc-field">
      <label htmlFor={fieldId}>{label}</label>
      {children({
        id: fieldId,
        "aria-describedby": description,
        "aria-invalid": error ? true : undefined,
      })}
      {hint && (
        <p id={`${fieldId}-hint`} className="sc-hint">
          {hint}
        </p>
      )}
      {error && (
        <p id={`${fieldId}-error`} className="sc-field-error">
          {error}
        </p>
      )}
    </div>
  );
}
