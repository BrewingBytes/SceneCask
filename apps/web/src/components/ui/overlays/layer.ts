"use client";
import { useSyncExternalStore, type KeyboardEvent as ReactKeyboardEvent } from "react";

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
      if (!element.getClientRects().length) return false;
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

// Ordered by nesting depth, then opening order, so a parent opened in the same commit
// as its nested child (child effects run first) still ends up beneath it.
const stack: { container: HTMLElement; depth: number }[] = [];
const madeInert = new Set<HTMLElement>();
let bodyStyle: { overflow: string; paddingRight: string } | null = null;
// Portals appended to body while a modal is open must become inert too.
let observer: MutationObserver | null = null;

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

/**
 * Register a modal layer at its nesting depth: everything outside the top layer
 * becomes inert and the page stops scrolling.
 */
export function pushLayer(container: HTMLElement, depth: number) {
  const index = stack.findIndex((layer) => layer.depth > depth);
  stack.splice(index < 0 ? stack.length : index, 0, { container, depth });
  sync();
  return () => {
    const index = stack.findIndex((layer) => layer.container === container);
    if (index >= 0) stack.splice(index, 1);
    sync();
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
