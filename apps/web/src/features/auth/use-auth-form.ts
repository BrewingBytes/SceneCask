"use client";
import { useCallback, useRef, useState } from "react";
import type { AuthFailure } from "./api";

/** Returned by a task that navigates away: the form stays busy until the page is replaced. */
export const LEAVING = Symbol("leaving");

/**
 * One submission at a time. A ref guards against Enter or a second click landing before the
 * busy state renders; the busy flag drives Button's own blocking and aria-busy.
 */
function useSubmit() {
  const inFlight = useRef(false);
  const [busy, setBusy] = useState(false);
  const run = useCallback(async (task: () => Promise<typeof LEAVING | void>) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    if ((await task()) === LEAVING) return;
    inFlight.current = false;
    setBusy(false);
  }, []);
  return [busy, run] as const;
}

/**
 * State every account form shares: per-field errors, one form-level problem and the submit
 * guard. Field errors move focus to the first invalid field, so it is read with its error.
 */
export function useAuthForm<Name extends string, Problem = AuthFailure>(initialProblem: Problem | null = null) {
  const form = useRef<HTMLFormElement>(null);
  const [errors, setErrors] = useState<Partial<Record<Name, string>>>({});
  const [problem, setProblem] = useState<Problem | null>(initialProblem);
  const [busy, run] = useSubmit();

  /** Replaces all field errors; true (and focus moved) when any field is invalid. */
  const showErrors = useCallback((next: Partial<Record<Name, string | undefined>>) => {
    setErrors(next as Partial<Record<Name, string>>);
    const invalid = Object.values(next).some(Boolean);
    if (invalid)
      requestAnimationFrame(() => form.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus());
    return invalid;
  }, []);

  const clearError = useCallback((name: Name) => setErrors((current) => ({ ...current, [name]: undefined })), []);

  return { form, errors, showErrors, clearError, problem, setProblem, busy, run };
}
