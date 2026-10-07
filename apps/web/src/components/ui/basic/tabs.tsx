"use client";
import { useEffect, useId, useRef, type ReactNode } from "react";
import "../../../styles";

export interface TabsProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  tabs: readonly {
    value: string;
    label: string;
    content: ReactNode;
    disabled?: boolean;
  }[];
}
export function Tabs({ label, value, onChange, tabs }: TabsProps) {
  const id = useId();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const available = tabs
    .map((tab, index) => (tab.disabled ? -1 : index))
    .filter((index) => index >= 0);
  const selected =
    available.find((index) => tabs[index].value === value) ?? available[0];
  const fallback = selected === undefined ? undefined : tabs[selected].value;
  const lastCorrection = useRef<string | null>(null);
  useEffect(() => {
    if (fallback === undefined || fallback === value) {
      lastCorrection.current = null;
      return;
    }
    const correction = JSON.stringify([value, fallback]);
    if (lastCorrection.current !== correction) {
      lastCorrection.current = correction;
      onChange(fallback);
    }
  }, [value, fallback, onChange]);
  const panel = selected ?? tabs.findIndex((tab) => tab.value === value);
  return (
    <div className="sc-tabs">
      <div
        role="tablist"
        aria-label={label}
        className="sc-tab-list"
        tabIndex={available.length ? undefined : 0}
        aria-disabled={available.length ? undefined : true}
      >
        {tabs.map((tab, index) => (
          <button
            key={tab.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="tab"
            id={`${id}-tab-${index}`}
            aria-controls={`${id}-panel-${index}`}
            aria-selected={panel === index}
            tabIndex={selected === index ? 0 : -1}
            disabled={tab.disabled}
            onClick={() => onChange(tab.value)}
            onKeyDown={(event) => {
              if (
                !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
              )
                return;
              event.preventDefault();
              const current = available.indexOf(index);
              const next =
                event.key === "Home"
                  ? available[0]
                  : event.key === "End"
                    ? available.at(-1)
                    : available[
                        (current +
                          (event.key === "ArrowRight" ? 1 : -1) +
                          available.length) %
                          available.length
                      ];
              if (next !== undefined) {
                refs.current[next]?.focus();
                onChange(tabs[next].value);
              }
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>
      {!available.length && <p role="status">No tabs are available.</p>}
      {tabs.map((tab, index) => (
        <div
          key={tab.value}
          role="tabpanel"
          id={`${id}-panel-${index}`}
          aria-labelledby={`${id}-tab-${index}`}
          hidden={panel !== index}
          tabIndex={0}
          className="sc-tab-panel"
        >
          {tab.content}
        </div>
      ))}
    </div>
  );
}
