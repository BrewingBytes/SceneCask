import type { ReactNode } from "react";

export interface SettingsSectionProps {
  id: string;
  title: string;
  lede?: ReactNode;
  children: ReactNode;
}

/** A D04 section: heading under a 3px ink rule. The heading takes focus when a row it held is gone. */
export function SettingsSection({ id, title, lede, children }: SettingsSectionProps) {
  return (
    <section className="sc-settings-section" aria-labelledby={`sc-settings-${id}`}>
      <h2 id={`sc-settings-${id}`} tabIndex={-1}>
        {title}
      </h2>
      {lede && <p className="sc-settings-lede">{lede}</p>}
      {children}
    </section>
  );
}

/** Raised-paper row: text on the left, its action aligned right, wrapping below on phones. */
export function SettingsRow({ title, detail, action }: { title: ReactNode; detail?: ReactNode; action?: ReactNode }) {
  return (
    <div className="sc-settings-row">
      <div className="sc-settings-row-main">
        <span className="sc-settings-row-title">{title}</span>
        {detail && <span className="sc-settings-row-detail">{detail}</span>}
      </div>
      {action && <div className="sc-settings-row-action">{action}</div>}
    </div>
  );
}

/**
 * A section's announcements. Both regions are mounted up front so later messages are read:
 * successes politely, failures as alerts that stay until the next attempt.
 */
export function SectionStatus({ status, problem }: { status?: ReactNode; problem?: ReactNode }) {
  return (
    <>
      <p className="sc-settings-status" role="status">
        {status}
      </p>
      <div className="sc-settings-problem" role="alert">
        {problem}
      </div>
    </>
  );
}
