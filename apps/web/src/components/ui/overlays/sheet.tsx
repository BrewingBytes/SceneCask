"use client";
import { Button } from "../basic/button";
import { ModalFrame, type ModalProps } from "./modal";

export interface SheetProps extends ModalProps {
  /** Accessible name of the header close button; default "Close". */
  closeLabel?: string;
}

/**
 * Taller task surface (for example add-to-library or set progress). A bottom sheet
 * below 860px and a centered 560px panel above it. The body scrolls; the footer stays visible.
 */
export function Sheet({ closeLabel = "Close", ...props }: SheetProps) {
  const { busy = false, dismissible = true, onOpenChange } = props;
  return (
    <ModalFrame
      {...props}
      role="dialog"
      variant="sheet"
      header={
        dismissible && (
          <Button
            variant="quiet"
            className="sc-modal-close"
            aria-label={closeLabel}
            aria-disabled={busy || undefined}
            onClick={() => onOpenChange(false)}
          >
            <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
              <path
                d="M18 6 6 18M6 6l12 12"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
            </svg>
          </Button>
        )
      }
    />
  );
}
