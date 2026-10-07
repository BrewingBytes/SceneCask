"use client";
import type { InputHTMLAttributes } from "react";
import { classes } from "./classes";
import "../../../styles/foundation.css";

export type CheckboxProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "type"
> & { label: string };
export function Checkbox({ label, className, ...props }: CheckboxProps) {
  return (
    <label className={classes("sc-choice", className)}>
      <input {...props} type="checkbox" />
      <span>{label}</span>
    </label>
  );
}
