"use client";
import {
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";

// Elements marked with this attribute (the toast viewport) stay reachable and announced while a modal is open.
export const PERSIST_ATTRIBUTE = "data-sc-overlay-persist";

const FOCUSABLE = [
  "a[href]",
  "area[href]",
  "button:not([disabled])",
  'input:not([disabled]):not([type="hidden"])',
  "select:not([disabled])",
  "textarea:not([disabled])",
  "iframe",
  "summary",
  "audio[controls]",
  "video[controls]",
  "[contenteditable]:not([contenteditable='false'])",
  "[tabindex]",
].join(",");

/** Tab-reachable elements in DOM order; only one radio per native group is reachable. */
export function tabbableWithin(root: HTMLElement): HTMLElement[] {
  const chosen = new Map<string, HTMLElement>();
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => {
      if (element.tabIndex < 0 || element.closest("[inert]")) return false;
      // :disabled also covers controls inside <fieldset disabled>.
      if (element.matches(":disabled")) return false;
      if (
        !(element.checkVisibility?.({ visibilityProperty: true }) ??
          element.getClientRects().length > 0)
      )
        return false;
      if (!(element instanceof HTMLInputElement) || element.type !== "radio")
        return true;
      if (!element.name) return true;
      if (!chosen.has(element.name))
        chosen.set(
          element.name,
          root.querySelector<HTMLInputElement>(
            `input[type="radio"][name="${CSS.escape(element.name)}"]:checked`,
          ) ?? element,
        );
      return chosen.get(element.name) === element;
    },
  );
}

/** Keep Tab and Shift+Tab inside the container, wrapping at either end. */
export function trapTab(event: KeyboardEvent | ReactKeyboardEvent, container: HTMLElement) {
  if (event.key !== "Tab") return;
  const items = tabbableWithin(container);
  const active = document.activeElement as HTMLElement | null;
  if (!items.length) {
    event.preventDefault();
    container.focus();
    return;
  }
  const first = items[0],
    last = items[items.length - 1];
  const outside = !active || !container.contains(active);
  if (event.shiftKey && (outside || active === first || active === container)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (outside || active === last)) {
    event.preventDefault();
    first.focus();
  }
}

export interface Layer {
  container: HTMLElement;
  /** The dialog panel, focused (or its first tabbable) when nothing better exists. */
  surface: HTMLElement;
  depth: number;
  /** Container of the enclosing modal in the React tree, if nested. */
  parent: HTMLElement | null;
  /** Focused before the layer opened, recorded before any nested layer moves focus. */
  previous: HTMLElement | null;
  fallback?: RefObject<HTMLElement | null>;
}

// Ordered by opening, except that a parent opened in the same commit as its nested
// child (child effects run first) is placed beneath that child.
const stack: Layer[] = [];
const madeInert = new Set<HTMLElement>();
let bodyStyle: { overflow: string; paddingRight: string } | null = null;
// Portals appended to body while a modal is open must become inert too.
let observer: MutationObserver | null = null;
// Layers closed in the current commit; focus is restored once after all cleanups.
const released: Layer[] = [];

function sync() {
  for (const element of madeInert) element.inert = false;
  madeInert.clear();
  const top = stack.at(-1)?.container;
  if (top)
    for (const child of Array.from(document.body.children)) {
      if (
        !(child instanceof HTMLElement) ||
        child === top ||
        child.inert ||
        child.hasAttribute(PERSIST_ATTRIBUTE) ||
        ["SCRIPT", "STYLE", "TEMPLATE"].includes(child.tagName)
      )
        continue;
      child.inert = true;
      madeInert.add(child);
    }
  if (stack.length && !observer) {
    observer = new MutationObserver(sync);
    observer.observe(document.body, { childList: true });
  } else if (!stack.length && observer) {
    observer.disconnect();
    observer = null;
  }
  if (stack.length && !bodyStyle) {
    const { style } = document.body;
    const scrollbar = innerWidth - document.documentElement.clientWidth;
    bodyStyle = { overflow: style.overflow, paddingRight: style.paddingRight };
    style.overflow = "hidden";
    if (scrollbar > 0)
      style.paddingRight = `calc(${getComputedStyle(document.body).paddingRight} + ${scrollbar}px)`;
  } else if (!stack.length && bodyStyle) {
    Object.assign(document.body.style, bodyStyle);
    bodyStyle = null;
  }
}

export const focusInto = (surface: HTMLElement) =>
  (tabbableWithin(surface)[0] ?? surface).focus();

const usable = (element: HTMLElement | null | undefined): element is HTMLElement =>
  !!element &&
  element.isConnected &&
  !element.closest("[inert]") &&
  !element.matches(":disabled");

function restoreFocus() {
  // A layer pushed again (React StrictMode remount) is not closed.
  const closed = released
    .splice(0)
    .filter((layer) => !stack.some((open) => open.container === layer.container))
    .sort((a, b) => a.depth - b.depth);
  if (!closed.length) return;
  const top = stack.at(-1);
  if (top?.container.contains(document.activeElement)) return;
  // The outermost closed layer wins, so closing a parent with its child returns to the page.
  for (const layer of closed)
    for (const candidate of [layer.previous, layer.fallback?.current])
      if (usable(candidate)) return candidate.focus();
  if (top) focusInto(top.surface);
}

/**
 * Register a modal layer on top of the stack: everything outside the top layer
 * becomes inert and the page stops scrolling. Releasing it restores focus to the
 * element focused before it opened, its fallback, or the layer now on top.
 */
export function pushLayer(layer: Layer) {
  const descends = (open: Layer) => {
    for (let parent = open.parent; parent; )
      if (parent === layer.container) return true;
      else parent = stack.find((item) => item.container === parent)?.parent ?? null;
    return false;
  };
  const index = stack.findIndex(descends);
  stack.splice(index < 0 ? stack.length : index, 0, layer);
  sync();
  return () => {
    const index = stack.indexOf(layer);
    if (index >= 0) stack.splice(index, 1);
    sync();
    if (!released.length) queueMicrotask(restoreFocus);
    released.push(layer);
  };
}

export const isTopLayer = (container: HTMLElement) =>
  stack.at(-1)?.container === container;

const noop = () => () => {};
/** False during server rendering and hydration, true afterwards; portals need document.body. */
export const useIsClient = () =>
  useSyncExternalStore(
    noop,
    () => true,
    () => false,
  );
