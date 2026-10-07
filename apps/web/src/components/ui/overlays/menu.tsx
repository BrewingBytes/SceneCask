"use client";
import Link from "next/link";
import {
  useEffect,
  useId,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { Button, type ButtonProps } from "../basic/button";
import { classes } from "../basic/classes";
import "../../../styles";
import "./overlays.css";

export interface MenuItem {
  /** Stable key, unique within the menu. */
  id: string;
  label: ReactNode;
  /** Runs after the menu closes and focus returns to the trigger. */
  onSelect?: () => void;
  /** Renders a navigation link instead of an action. */
  href?: string;
  /** Stays focusable and announced, but cannot be selected. */
  disabled?: boolean;
  tone?: "default" | "destructive";
  /** Present for toggles: renders menuitemcheckbox with aria-checked. */
  checked?: boolean;
}

export interface MenuProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Accessible name of the menu, also the trigger name unless triggerLabel is set. */
  label: string;
  triggerLabel?: string;
  /** Visible trigger content, for example an Avatar or an icon. */
  trigger: ReactNode;
  triggerVariant?: ButtonProps["variant"];
  items: MenuItem[];
  /** Non-interactive context shown above the items, such as the account name. */
  header?: ReactNode;
  /** Edge of the trigger the popover aligns to; default end. */
  align?: "start" | "end";
  className?: string;
}

/**
 * Menu button following the WAI-ARIA pattern: Enter/Space/ArrowDown open on the
 * first item, ArrowUp on the last; arrows wrap, Home/End and type-ahead move focus;
 * Escape or Tab closes and returns focus to the trigger; outside presses close.
 */
export function Menu({
  open,
  onOpenChange,
  label,
  triggerLabel,
  trigger,
  triggerVariant = "quiet",
  items,
  header,
  align = "end",
  className,
}: MenuProps) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const entry = useRef<"first" | "last">("first");
  const typed = useRef({ text: "", at: 0 });

  const menuItems = () =>
    Array.from(
      list.current?.querySelectorAll<HTMLElement>("[data-sc-menu-item]") ?? [],
    );
  const focusItem = (index: number) => {
    const all = menuItems();
    if (all.length) all[(index + all.length) % all.length].focus();
  };
  const close = (restore: boolean) => {
    onOpenChange(false);
    if (restore)
      root.current?.querySelector<HTMLElement>("[aria-haspopup=menu]")?.focus();
  };

  useEffect(() => {
    if (!open) return;
    focusItem(entry.current === "last" ? -1 : 0);
    entry.current = "first";
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) onOpenChange(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
    // Focus moves once per opening.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const openAt = (position: "first" | "last") => {
    entry.current = position;
    if (open) focusItem(position === "last" ? -1 : 0);
    else onOpenChange(true);
  };

  const onTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      openAt(event.key === "ArrowUp" ? "last" : "first");
    } else if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const all = menuItems();
    const index = all.indexOf(document.activeElement as HTMLElement);
    const moves: Record<string, number> = {
      ArrowDown: index + 1,
      ArrowUp: index - 1,
      Home: 0,
      End: -1,
    };
    if (event.key in moves) {
      event.preventDefault();
      focusItem(moves[event.key]);
    } else if (event.key === "Escape") {
      // Stop here so an enclosing dialog stays open.
      event.preventDefault();
      event.stopPropagation();
      close(true);
    } else if (event.key === "Tab") {
      // Stop here too: an enclosing modal's Tab trap must not move focus off the trigger.
      event.preventDefault();
      event.stopPropagation();
      close(true);
    } else if (event.key === " " && event.target instanceof HTMLAnchorElement) {
      // Links ignore Space natively; menu items activate on both Enter and Space.
      event.preventDefault();
      event.target.click();
    } else if (
      event.key.length === 1 &&
      event.key !== " " &&
      !event.altKey &&
      !event.ctrlKey &&
      !event.metaKey
    ) {
      const now = event.timeStamp;
      typed.current = {
        text: (now - typed.current.at > 500 ? "" : typed.current.text) + event.key.toLowerCase(),
        at: now,
      };
      // A longer prefix may still match the current item, so search from it.
      const start = typed.current.text.length > 1 ? index : index + 1;
      const order = [...all.slice(start), ...all.slice(0, start)];
      const match =
        order.find((item) =>
          item.textContent?.trim().toLowerCase().startsWith(typed.current.text),
        ) ?? null;
      match?.focus();
    }
  };

  const select = (item: MenuItem) => {
    if (item.disabled) return;
    close(true);
    item.onSelect?.();
  };

  return (
    <div
      ref={root}
      className={classes("sc-menu", className)}
      onBlur={(event) => {
        // Closing when focus leaves keeps a stale popover from lingering behind other content.
        if (open && !root.current?.contains(event.relatedTarget as Node | null) && event.relatedTarget)
          onOpenChange(false);
      }}
    >
      <Button
        variant={triggerVariant}
        aria-label={triggerLabel ?? label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? `${id}-menu` : undefined}
        onClick={() => (open ? onOpenChange(false) : openAt("first"))}
        onKeyDown={onTriggerKeyDown}
      >
        {trigger}
      </Button>
      {open && (
        <div
          ref={list}
          id={`${id}-menu`}
          role="menu"
          aria-label={label}
          className={classes("sc-menu-popover", `sc-menu-${align}`)}
          onKeyDown={onMenuKeyDown}
        >
          {header && (
            <div className="sc-menu-header" role="none">
              {header}
            </div>
          )}
          {items.map((item) => {
            const shared = {
              "data-sc-menu-item": "",
              role: item.checked === undefined ? "menuitem" : "menuitemcheckbox",
              "aria-checked": item.checked,
              "aria-disabled": item.disabled || undefined,
              tabIndex: -1,
              className: classes(
                "sc-menu-item",
                item.tone === "destructive" && "sc-menu-item-destructive",
              ),
            };
            if (item.href && !item.disabled)
              return (
                <Link
                  key={item.id}
                  {...shared}
                  href={item.href}
                  onClick={() => select(item)}
                >
                  {item.label}
                </Link>
              );
            return (
              <button
                key={item.id}
                type="button"
                {...shared}
                onClick={() => select(item)}
              >
                <span>{item.label}</span>
                {item.checked !== undefined && (
                  <svg
                    className="sc-menu-check"
                    viewBox="0 0 24 24"
                    width="20"
                    height="20"
                    aria-hidden="true"
                  >
                    {item.checked && (
                      <path
                        d="M20 6 9 17l-5-5"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    )}
                  </svg>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
