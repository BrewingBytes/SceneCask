"use client";
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { classes } from "../basic/classes";
import {
  focusInto,
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

// The enclosing modal, so nested layers stack above their parents.
const ParentLayer = createContext<{
  depth: number;
  container: RefObject<HTMLDivElement | null> | null;
}>({ depth: 0, container: null });

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
  const parentLayer = useContext(ParentLayer);
  const depth = parentLayer.depth;
  const scrim = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const pressStartedOnScrim = useRef(false);
  const cancellable = dismissible && !busy;

  const previous = useRef<HTMLElement | null>(null);
  const cancel = useRef(() => {});
  useEffect(() => {
    cancel.current = () => {
      if (cancellable) onOpenChange(false);
    };
  });

  // Layout effects all run before any passive effect, so this records the trigger
  // even when a nested layer opening in the same commit moves focus first.
  useLayoutEffect(() => {
    // Recorded once: a StrictMode remount must not replace it with a nested layer's control.
    const active = document.activeElement;
    if (
      !previous.current &&
      active instanceof HTMLElement &&
      active !== document.body &&
      !scrim.current!.contains(active)
    )
      previous.current = active;
  }, []);

  useEffect(() => {
    const container = scrim.current!,
      surface = panel.current!;
    const release = pushLayer({
      container,
      surface,
      depth,
      parent: parentLayer.container?.current ?? null,
      previous: previous.current,
      fallback: returnFocusRef,
    });
    const target =
      initialFocusRef?.current ??
      surface.querySelector<HTMLElement>("[data-autofocus]") ??
      tabbableWithin(surface.querySelector(".sc-modal-body") ?? surface)[0];
    // A nested layer opened in the same commit is already on top and keeps focus.
    if (isTopLayer(container)) {
      if (target) target.focus();
      else focusInto(surface);
    }
    // Focus can only escape through programmatic moves; pull it back into the top layer.
    const guard = (event: FocusEvent) => {
      const next = event.target as Node;
      if (
        isTopLayer(container) &&
        !container.contains(next) &&
        !(next instanceof Element && next.closest(`[${PERSIST_ATTRIBUTE}]`))
      )
        focusInto(surface);
    };
    // Listen on window so Escape and Tab work even when focus has fallen to body.
    // Inner components (Menu) that handle a key call preventDefault first.
    const keys = (event: KeyboardEvent) => {
      if (!isTopLayer(container) || event.defaultPrevented) return;
      // Escape during IME composition cancels the composition, not the modal.
      if (event.isComposing) return;
      // Keys pressed in the toast viewport belong to the toast, not the modal.
      if (
        event.key === "Escape" &&
        event.target instanceof Element &&
        event.target.closest(`[${PERSIST_ATTRIBUTE}]`)
      )
        return;
      if (event.key === "Escape") {
        event.preventDefault();
        cancel.current();
      } else trapTab(event, surface);
    };
    document.addEventListener("focusin", guard);
    window.addEventListener("keydown", keys);
    return () => {
      document.removeEventListener("focusin", guard);
      window.removeEventListener("keydown", keys);
      release();
    };
    // Focus moves once per opening; later prop changes must not steal focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      ref={scrim}
      className={classes("sc-overlay", `sc-overlay-${variant}`)}
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
        {/* Header, body and footer all nest: a dialog opened from any of them stacks above. */}
        <ParentLayer.Provider value={{ depth: depth + 1, container: scrim }}>
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
        {children && <div className="sc-modal-body">{children}</div>}
        {footer && <div className="sc-modal-footer sc-actions">{footer}</div>}
        </ParentLayer.Provider>
      </div>
    </div>
  );
}
