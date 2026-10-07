"use client";
import type { ReactNode } from "react";
import { Button } from "../basic/button";
import { ModalFrame, type ModalProps } from "./modal";

export interface DialogProps extends ModalProps {
  /** Use alertdialog for confirmations that interrupt the current task. */
  role?: "dialog" | "alertdialog";
}

/** Centered on desktop, rising from the bottom edge below 860px. */
export function Dialog({ role = "dialog", ...props }: DialogProps) {
  return <ModalFrame {...props} role={role} variant="dialog" />;
}

export interface ConfirmDialogProps
  extends Omit<DialogProps, "footer" | "role"> {
  confirmLabel: string;
  cancelLabel?: string;
  onConfirm: () => void;
  /** Destructive styling for irreversible actions such as erasing history. */
  destructive?: boolean;
  confirmDisabled?: boolean;
  /** Safe product copy for a failed attempt; persists until the caller clears it. */
  error?: ReactNode;
}

/**
 * Alert dialog with Cancel and one confirm action. Cancel has initial focus. While
 * busy the confirm button blocks repeat activation and every dismissal is blocked.
 */
export function ConfirmDialog({
  confirmLabel,
  cancelLabel = "Cancel",
  onConfirm,
  destructive = false,
  confirmDisabled = false,
  error,
  children,
  busy = false,
  ...props
}: ConfirmDialogProps) {
  return (
    <Dialog
      {...props}
      busy={busy}
      role="alertdialog"
      footer={
        <>
          <Button
            variant="secondary"
            data-autofocus
            aria-disabled={busy || undefined}
            onClick={() => props.onOpenChange(false)}
          >
            {cancelLabel}
          </Button>
          <Button
            variant={destructive ? "destructive" : "primary"}
            busy={busy}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
      {/* Mounted with the dialog so a later failure is announced when it appears. */}
      <div role="alert" className="sc-modal-error">
        {error}
      </div>
    </Dialog>
  );
}
