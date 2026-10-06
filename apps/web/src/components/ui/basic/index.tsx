"use client";

import {
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type TextareaHTMLAttributes,
  type ReactNode,
  type CSSProperties,
} from "react";
import "../../../styles/foundation.css";

const classes = (...names: (string | undefined | false)[]) =>
  names.filter(Boolean).join(" ");
export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "quiet" | "destructive";
  busy?: boolean;
}
export function Button({
  variant = "primary",
  busy = false,
  disabled,
  children,
  className,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      {...props}
      type={type}
      className={classes("sc-button", `sc-button-${variant}`, className)}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
    >
      {children}
      {busy && (
        <span className="sc-busy" aria-hidden="true">
          {" "}
          ···
        </span>
      )}
    </button>
  );
}
interface FieldInfo {
  label: string;
  hint?: string;
  error?: string;
}
export type TextFieldProps = InputHTMLAttributes<HTMLInputElement> & FieldInfo;
export function TextField({
  label,
  hint,
  error,
  id,
  className,
  "aria-describedby": describedBy,
  ...props
}: TextFieldProps) {
  const generated = useId();
  const fieldId = id ?? generated;
  return (
    <div className="sc-field">
      <label htmlFor={fieldId}>{label}</label>
      <input
        {...props}
        id={fieldId}
        className={classes("sc-input", className)}
        aria-invalid={!!error || undefined}
        aria-describedby={
          classes(
            describedBy,
            hint && `${fieldId}-hint`,
            error && `${fieldId}-error`,
          ) || undefined
        }
      />
      {hint && (
        <p id={`${fieldId}-hint`} className="sc-hint">
          {hint}
        </p>
      )}
      {error && (
        <p id={`${fieldId}-error`} className="sc-field-error">
          {error}
        </p>
      )}
    </div>
  );
}
export type TextAreaProps = TextareaHTMLAttributes<HTMLTextAreaElement> &
  FieldInfo;
export function TextArea({
  label,
  hint,
  error,
  id,
  className,
  "aria-describedby": describedBy,
  ...props
}: TextAreaProps) {
  const generated = useId();
  const fieldId = id ?? generated;
  return (
    <div className="sc-field">
      <label htmlFor={fieldId}>{label}</label>
      <textarea
        {...props}
        id={fieldId}
        className={classes("sc-input", className)}
        aria-invalid={!!error || undefined}
        aria-describedby={
          classes(
            describedBy,
            hint && `${fieldId}-hint`,
            error && `${fieldId}-error`,
          ) || undefined
        }
      />
      {hint && (
        <p id={`${fieldId}-hint`} className="sc-hint">
          {hint}
        </p>
      )}
      {error && (
        <p id={`${fieldId}-error`} className="sc-field-error">
          {error}
        </p>
      )}
    </div>
  );
}
export type CheckboxProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "type"
> & { label: string };
export function Checkbox({ label, className, ...props }: CheckboxProps) {
  return (
    <label className={classes("sc-choice", className)}>
      <input {...props} type="checkbox" />
      <span>{label}</span>
    </label>
  );
}
export interface RadioGroupProps {
  label: string;
  name: string;
  value: string;
  onChange: (value: string) => void;
  options: readonly { value: string; label: string; disabled?: boolean }[];
  disabled?: boolean;
}
export function RadioGroup({
  label,
  name,
  value,
  onChange,
  options,
  disabled,
}: RadioGroupProps) {
  return (
    <fieldset className="sc-radio" disabled={disabled}>
      <legend>{label}</legend>
      {options.map((option) => (
        <label key={option.value} className="sc-choice">
          <input
            type="radio"
            name={name}
            value={option.value}
            checked={value === option.value}
            onChange={() => onChange(option.value)}
            disabled={option.disabled}
          />
          <span>{option.label}</span>
        </label>
      ))}
    </fieldset>
  );
}
export interface BadgeProps {
  children: ReactNode;
  tone?: "neutral" | "accent" | "error";
}
export function Badge({ children, tone = "neutral" }: BadgeProps) {
  return <span className={`sc-badge sc-badge-${tone}`}>{children}</span>;
}
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
  return (
    <div className="sc-tabs">
      <div role="tablist" aria-label={label} className="sc-tab-list">
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
            aria-selected={value === tab.value}
            tabIndex={value === tab.value ? 0 : -1}
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
      {tabs.map((tab, index) => (
        <div
          key={tab.value}
          role="tabpanel"
          id={`${id}-panel-${index}`}
          aria-labelledby={`${id}-tab-${index}`}
          hidden={value !== tab.value}
          tabIndex={0}
          className="sc-tab-panel"
        >
          {tab.content}
        </div>
      ))}
    </div>
  );
}
export interface ProgressProps {
  value: number;
  max?: number;
  label: string;
}
export function Progress({ value, max = 100, label }: ProgressProps) {
  const total = Number.isFinite(max) && max > 0 ? max : 100;
  const current = Number.isFinite(value)
    ? Math.max(0, Math.min(total, value))
    : 0;
  return (
    <div
      className="sc-progress"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={current}
    >
      <span style={{ width: `${(current / total) * 100}%` }} />
    </div>
  );
}
export interface PosterProps {
  src?: string | null;
  alt: string;
  aspect?: "poster" | "still";
  className?: string;
}
export function Poster({
  src,
  alt,
  aspect = "poster",
  className,
}: PosterProps) {
  const [failedSource, setFailedSource] = useState<string | null>(null);
  const hasImage = !!src && failedSource !== src;
  return (
    <div className={classes("sc-art", `sc-art-${aspect}`, className)}>
      {hasImage ? (
        // Native image keeps failure handling and reserved dimensions together; callers supply authorized artwork only.
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={src}
          alt={alt}
          width={aspect === "poster" ? 200 : 320}
          height={aspect === "poster" ? 300 : 180}
          onError={() => setFailedSource(src)}
        />
      ) : (
        <div
          className="sc-art-fallback"
          role={alt ? "img" : undefined}
          aria-label={alt || undefined}
          aria-hidden={!alt || undefined}
        >
          <span aria-hidden="true">SC</span>
        </div>
      )}
    </div>
  );
}
export interface AvatarProps {
  initials: string;
  label: string;
}
export function Avatar({ initials, label }: AvatarProps) {
  return (
    <span className="sc-avatar" role="img" aria-label={label}>
      {initials.slice(0, 3)}
    </span>
  );
}
export interface SkeletonProps {
  label?: string;
  width?: CSSProperties["width"];
  height?: CSSProperties["height"];
}
export function Skeleton({
  label = "Loading",
  width = "100%",
  height = "1em",
}: SkeletonProps) {
  return (
    <span className="sc-skeleton" role="status" style={{ width, height }}>
      <span className="sc-sr-only">{label}</span>
    </span>
  );
}
export interface EmptyStateProps {
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}
export function EmptyState({ title, children, action }: EmptyStateProps) {
  return (
    <div className="sc-empty">
      <h3>{title}</h3>
      {children && <div className="sc-state-copy">{children}</div>}
      {action}
    </div>
  );
}
export interface ErrorStateProps {
  title?: string;
  children?: ReactNode;
  onRetry?: () => void;
  onDismiss?: () => void;
  busy?: boolean;
}
export function ErrorState({
  title = "Something went wrong",
  children,
  onRetry,
  onDismiss,
  busy,
}: ErrorStateProps) {
  return (
    <div className="sc-error">
      <div role="alert">
        <h3>{title}</h3>
        {children && <div className="sc-state-copy">{children}</div>}
      </div>
      <div className="sc-actions">
        {onRetry && (
          <Button variant="secondary" onClick={onRetry} busy={busy}>
            Retry
          </Button>
        )}
        {onDismiss && (
          <Button variant="quiet" onClick={onDismiss}>
            Dismiss
          </Button>
        )}
      </div>
    </div>
  );
}
