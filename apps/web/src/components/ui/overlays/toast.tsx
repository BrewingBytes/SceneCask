"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "../basic/button";
import { classes } from "../basic/classes";
import { PERSIST_ATTRIBUTE, useIsClient } from "./layer";
import "../../../styles";
import "./overlays.css";

/** D01 and the handoff motion token: success toasts last 6s; errors persist. */
export const TOAST_DURATION_MS = 6000;

export type ToastTone = "success" | "info" | "error";
export type ToastDismissReason = "timeout" | "dismiss";

export interface ToastMessage {
  id: string;
  tone: ToastTone;
  /** Safe product copy only: never provider payloads, protected strings or image paths. */
  message: string;
  onUndo?: () => void;
  onRetry?: () => void;
  /** One additional caller-owned action, for example Discuss. */
  action?: { label: string; onAction: () => void };
  /** The in-flight action keeps the toast open and blocks repeat activation. */
  busy?: "undo" | "retry" | "action";
  /** Success/info toasts that must wait for an explicit Dismiss. Errors always persist. */
  persistent?: boolean;
  /**
   * Changes when the same toast is shown again: restarts its 6s and re-announces it.
   * useToastQueue sets it on every show(); without it, only new copy restarts.
   */
  version?: number;
}

export interface ToastViewportProps {
  toasts: ToastMessage[];
  /** Remove the toast: after the timeout (success/info) or the Dismiss button. */
  onDismiss: (id: string, reason: ToastDismissReason) => void;
  label?: string;
  /** Lift toasts above the 72px mobile navigation bar; default true. */
  aboveBottomNav?: boolean;
}

/**
 * Persistent live regions for toasts. Mount once near the application root so the
 * regions exist before the first message. Success/info are polite statuses that
 * expire after 6s (paused while hovered, focused or busy); errors are assertive
 * alerts that stay until Retry succeeds or the person dismisses them.
 */
export function ToastViewport({
  toasts,
  onDismiss,
  label = "Notifications",
  aboveBottomNav = true,
}: ToastViewportProps) {
  const client = useIsClient();
  const statuses = toasts.filter((toast) => toast.tone !== "error");
  const errors = toasts.filter((toast) => toast.tone === "error");
  const viewport = (
    <section
      aria-label={label}
      {...{ [PERSIST_ATTRIBUTE]: "" }}
      className={classes(
        "sc-toast-viewport",
        aboveBottomNav && "sc-toast-viewport-nav",
      )}
    >
      <div role="status" className="sc-toast-region">
        {statuses.map((toast) => (
          <ToastCard
            key={`${toast.id}:${toast.version ?? toast.message}`}
            toast={toast}
            onDismiss={onDismiss}
          />
        ))}
      </div>
      <div role="alert" className="sc-toast-region">
        {errors.map((toast) => (
          <ToastCard
            key={`${toast.id}:${toast.version ?? toast.message}`}
            toast={toast}
            onDismiss={onDismiss}
          />
        ))}
      </div>
    </section>
  );
  // Portaled outside the page so a modal layer does not make toasts inert.
  return client ? createPortal(viewport, document.body) : null;
}

function ToastCard({
  toast,
  onDismiss,
}: {
  toast: ToastMessage;
  onDismiss: ToastViewportProps["onDismiss"];
}) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const remaining = useRef(TOAST_DURATION_MS);
  const expires = toast.tone !== "error" && !toast.persistent;
  const card = useRef<HTMLDivElement>(null);

  // Parents often pass an inline onDismiss; a ref keeps re-renders from restarting timers.
  const dismiss = useRef(onDismiss);
  useEffect(() => {
    dismiss.current = onDismiss;
  });
  // Removing the focused action fires no blur, so recheck focus when actions change.
  const actions = [!!toast.onUndo, !!toast.onRetry, !!toast.action].join();

  useEffect(() => {
    if (!expires || hovered || toast.busy) return;
    // Only real focus inside pauses; a stale flag from a removed action does not.
    if (focused && card.current?.contains(document.activeElement)) return;
    const started = Date.now();
    const timer = setTimeout(
      () => dismiss.current(toast.id, "timeout"),
      remaining.current,
    );
    return () => {
      clearTimeout(timer);
      remaining.current = Math.max(
        0,
        remaining.current - (Date.now() - started),
      );
    };
  }, [expires, hovered, focused, toast.busy, toast.id, actions]);

  return (
    <div
      ref={card}
      className={classes("sc-toast", `sc-toast-${toast.tone}`)}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null))
          setFocused(false);
      }}
    >
      <p className="sc-toast-message">{toast.message}</p>
      <div className="sc-toast-actions">
        {toast.onUndo && (
          <Button
            variant="secondary"
            className="sc-toast-action"
            busy={toast.busy === "undo"}
            onClick={toast.onUndo}
          >
            Undo
          </Button>
        )}
        {toast.action && (
          <Button
            variant="secondary"
            className="sc-toast-action"
            busy={toast.busy === "action"}
            onClick={toast.action.onAction}
          >
            {toast.action.label}
          </Button>
        )}
        {toast.onRetry && (
          <Button
            variant="secondary"
            className="sc-toast-action"
            busy={toast.busy === "retry"}
            onClick={toast.onRetry}
          >
            Retry
          </Button>
        )}
        <Button
          variant="quiet"
          className="sc-toast-action sc-toast-dismiss"
          aria-disabled={toast.busy ? true : undefined}
          onClick={() => onDismiss(toast.id, "dismiss")}
        >
          Dismiss
        </Button>
      </div>
    </div>
  );
}

export type ToastInput = Omit<ToastMessage, "id"> & { id?: string };

/**
 * Minimal client state for ToastViewport. It holds messages only; callers own the
 * mutations behind Undo/Retry and update or dismiss toasts with their outcome.
 * At most `limit` success/info toasts are kept (oldest dropped); errors are never dropped.
 */
export function useToastQueue(limit = 3) {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const counter = useRef(0);
  const show = useCallback(
    (input: ToastInput) => {
      const id = input.id ?? `toast-${++counter.current}`;
      const version = ++counter.current;
      setToasts((current) => {
        const next = [...current.filter((toast) => toast.id !== id), { ...input, id, version }];
        let excess =
          next.filter((toast) => toast.tone !== "error").length - limit;
        return next.filter(
          (toast) => toast.tone === "error" || excess-- <= 0,
        );
      });
      return id;
    },
    [limit],
  );
  const update = useCallback(
    (id: string, patch: Partial<Omit<ToastMessage, "id">>) =>
      setToasts((current) =>
        current.map((toast) => (toast.id === id ? { ...toast, ...patch } : toast)),
      ),
    [],
  );
  const dismiss = useCallback(
    (id: string) =>
      setToasts((current) => current.filter((toast) => toast.id !== id)),
    [],
  );
  return { toasts, show, update, dismiss };
}
