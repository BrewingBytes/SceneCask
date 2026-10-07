"use client";
import type { InputHTMLAttributes } from "react";
import type { FieldInfo } from "./field";
import { renderTextControl } from "./text-control";
export type TextFieldProps = InputHTMLAttributes<HTMLInputElement> & FieldInfo;
export function TextField(props: TextFieldProps) {
  return renderTextControl(props, (control) => <input {...control} />);
}
