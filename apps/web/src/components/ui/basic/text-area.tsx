"use client";
import type { TextareaHTMLAttributes } from "react";
import type { FieldInfo } from "./field";
import { renderTextControl } from "./text-control";
export type TextAreaProps = TextareaHTMLAttributes<HTMLTextAreaElement> &
  FieldInfo;
export function TextArea(props: TextAreaProps) {
  return renderTextControl(props, (control) => <textarea {...control} />);
}
