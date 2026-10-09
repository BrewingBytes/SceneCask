"use client";
import { TextField, type TextFieldProps } from "../../components/ui/basic";
import { PASSWORD_RULE } from "./validation";

type FieldProps = Omit<TextFieldProps, "label" | "type" | "value" | "onChange"> & {
  label?: string;
  value: string;
  onValue: (value: string) => void;
};

export function EmailField({ label = "Email", onValue, ...props }: FieldProps) {
  return (
    <TextField
      label={label}
      type="email"
      autoComplete="email"
      autoCapitalize="none"
      spellCheck={false}
      inputMode="email"
      {...props}
      onChange={(event) => onValue(event.target.value)}
    />
  );
}

/** `new` shows the C04 length rule; `confirm` repeats a new password. Values are never trimmed. */
export function PasswordField({
  label = "Password",
  purpose,
  onValue,
  ...props
}: FieldProps & { purpose: "current" | "new" | "confirm" }) {
  return (
    <TextField
      label={label}
      type="password"
      autoComplete={purpose === "current" ? "current-password" : "new-password"}
      hint={purpose === "new" ? PASSWORD_RULE : undefined}
      {...props}
      onChange={(event) => onValue(event.target.value)}
    />
  );
}
