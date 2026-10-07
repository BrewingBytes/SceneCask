"use client";
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { classes } from "../basic/classes";
import {
  isTopLayer,
  PERSIST_ATTRIBUTE,
  pushLayer,
  tabbableWithin,
  trapTab,
  useIsClient,
} from "./layer";
import "../../../styles";
import "./overlays.css";

/** Inputs shared by Dialog and Sheet. */
export interface ModalProps {
  open: boolean;
  /** Called with false when the person cancels (Escape, scrim, Cancel/Close). */
  onOpenChange: (open: boolean) => void;
  /** Visible heading and accessible name. Never pass protected episode details. */
  title: ReactNode;
  description?: ReactNode;
  kicker?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  /** Escape and scrim dismissal; default true. Footer buttons can still close. */
  dismissible?: boolean;
  /** An in-flight mutation: blocks every dismissal so it is never silently discarded. */
  busy?: boolean;
  /** Receives focus on open instead of the first [data-autofocus] or tabbable element. */
  initialFocusRef?: RefObject<HTMLElement | null>;
  /** Receives focus on close when the element focused at open no longer exists. */
  returnFocusRef?: RefObject<HTMLElement | null>;
  className?: string;
}

// Nesting depth of the enclosing modal, so nested layers stack above their parents.
const LayerDepth = createContext(0);

interface FrameProps extends ModalProps {
  variant: "dialog" | "sheet";
  role: "dialog" | "alertdialog";
  header?: ReactNode;
}

export function ModalFrame(props: FrameProps) {
  const client = useIsClient();
  if (!props.open || !client) return null;
  return createPortal(<OpenModal {...props} />, document.body);
}

function OpenModal({
  onOpenChange,
  title,
  description,
  kicker,
  children,
  footer,
  dismissible = true,
  busy = false,
  initialFocusRef,
  returnFocusRef,
  className,
  variant,
  role,
  header,
}: FrameProps) {
  const id = useId();
  const depth = useContext(LayerDepth);
  const scrim = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const pressStartedOnScrim = useRef(false);
  const cancellable = dismissible && !busy;

  useEffect(() => {
    const container = scrim.current!,
      surface = panel.current!;
    const previous =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const fallback = returnFocusRef;
    const release = pushLayer(container, depth);
    const target =
      initialFocusRef?.current ??
      surface.querySelector<HTMLElement>("[data-autofocus]") ??
      tabbableWithin(surface.querySelector(".sc-modal-body") ?? surface)[0] ??
      tabbableWithin(surface)[0] ??
      surface;
    target.focus();
    // Focus can only escape through programmatic moves; pull it back into the top layer.
    const guard = (event: FocusEvent) => {
      const next = event.target as Node;
      if (
        isTopLayer(container) &&
        !container.contains(next) &&
        !(next instanceof Element && next.closest(`[${PERSIST_ATTRIBUTE}]`))
      )
        (tabbableWithin(surface)[0] ?? surface).focus();
    };
    document.addEventListener("focusin", guard);
    return () => {
      document.removeEventListener("focusin", guard);
      release();
      const destination = previous?.isConnected ? previous : fallback?.current;
      destination?.focus();
    };
    // Focus moves once per opening; later prop changes must not steal focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!isTopLayer(scrim.current!)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (cancellable) onOpenChange(false);
    } else trapTab(event, panel.current!);
  };

  return (
    <div
      ref={scrim}
      className={classes("sc-overlay", `sc-overlay-${variant}`)}
      onKeyDown={onKeyDown}
      onPointerDown={(event) => {
        pressStartedOnScrim.current = event.target === event.currentTarget;
      }}
      onClick={(event) => {
        // Only a press that starts and ends on the scrim dismisses; text selection drags do not.
        if (
          event.target === event.currentTarget &&
          pressStartedOnScrim.current &&
          cancellable
        )
          onOpenChange(false);
        pressStartedOnScrim.current = false;
      }}
    >
      <div
        ref={panel}
        role={role}
        aria-modal="true"
        aria-labelledby={`${id}-title`}
        aria-describedby={description ? `${id}-description` : undefined}
        aria-busy={busy || undefined}
        tabIndex={-1}
        className={classes("sc-modal", `sc-modal-${variant}`, className)}
      >
        <div className="sc-modal-header">
          <div className="sc-modal-heading">
            {kicker && <p className="sc-kicker">{kicker}</p>}
            <h2 id={`${id}-title`} className="sc-modal-title">
              {title}
            </h2>
          </div>
          {header}
        </div>
        {description && (
          <div id={`${id}-description`} className="sc-modal-description">
            {description}
          </div>
        )}
        {children && (
          <LayerDepth.Provider value={depth + 1}>
            <div className="sc-modal-body">{children}</div>
          </LayerDepth.Provider>
        )}
        {footer && <div className="sc-modal-footer sc-actions">{footer}</div>}
      </div>
    </div>
  );
}
