import type { AriaAttributes, ReactNode } from "react";
import { Field, type FieldControlProps, type FieldInfo } from "./field";
import { classes } from "./classes";

type TextControlAttributes = FieldInfo & {
  id?: string;
  className?: string;
  "aria-describedby"?: string;
  "aria-invalid"?: AriaAttributes["aria-invalid"];
};
type FieldKeys = keyof TextControlAttributes;
/** Shared adapter for native text controls; accessibility is owned only by Field. */
export function renderTextControl<T extends TextControlAttributes>(
  {
    label,
    hint,
    error,
    id,
    className,
    "aria-describedby": describedBy,
    "aria-invalid": invalid,
    ...native
  }: T,
  render: (
    props: Omit<T, FieldKeys> & FieldControlProps & { className: string },
  ) => ReactNode,
) {
  return (
    <Field
      label={label}
      hint={hint}
      error={error}
      id={id}
      describedBy={describedBy}
      invalid={invalid}
    >
      {(control) =>
        render({
          ...native,
          ...control,
          className: classes("sc-input", className),
        })
      }
    </Field>
  );
}
