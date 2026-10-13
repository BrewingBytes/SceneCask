"use client";
import { useState, type FormEvent } from "react";
import { Avatar, Button, TextField } from "../../../components/ui/basic";
import { endExpiredSession } from "../../auth";
import { LEAVING, useAuthForm } from "../../auth/use-auth-form";
import { displayNameError, HANDLE_RULE, handleError, initialsOf } from "../../auth/validation";
import type { Failure, SettingsApi, User } from "./api";
import { settingsFailureCopy } from "./copy";
import { SectionStatus, SettingsSection } from "./section";

export interface ProfileSectionProps {
  api: SettingsApi;
  user: User;
  onSaved: (user: User) => void;
}

/**
 * D04 Profile: edit the display name. The handle is read-only once chosen (C04); an account
 * without one may choose it here, and it is fixed after saving.
 */
export function ProfileSection({ api, user, onSaved }: ProfileSectionProps) {
  const [displayName, setDisplayName] = useState(user.displayName ?? "");
  const [handle, setHandle] = useState("");
  const [saved, setSaved] = useState("");
  const { form, errors, showErrors, clearError, problem, setProblem, busy, run } = useAuthForm<
    "displayName" | "handle",
    Failure
  >();
  const chosen = user.handle;

  function rejected(failure: Failure) {
    if (failure.kind === "handle_taken") return showErrors({ handle: "That handle is taken. Try another." });
    if (failure.kind !== "invalid") return false;
    return showErrors({
      displayName: failure.fields.includes("displayName") ? "Use 1–80 characters." : undefined,
      // A chosen handle is read-only; a rejection of it falls through to the form-level problem.
      handle: !chosen && failure.fields.includes("handle") ? `Use ${HANDLE_RULE}` : undefined,
    });
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setProblem(null);
    setSaved("");
    const next = { displayName: displayNameError(displayName), handle: chosen ? undefined : handleError(handle) };
    if (showErrors(next)) return;
    await run(async () => {
      const result = await api.updateProfile(displayName.trim(), chosen ?? handle);
      if (result.ok) {
        onSaved(result.data);
        setDisplayName(result.data.displayName ?? "");
        return setSaved(chosen ? "Profile saved." : `Profile saved. Your handle is @${result.data.handle}.`);
      }
      if (result.failure.kind === "auth") {
        endExpiredSession();
        return LEAVING;
      }
      if (!rejected(result.failure)) setProblem(result.failure);
    });
  }

  const initials = initialsOf(displayName);
  return (
    <SettingsSection id="profile" title="Profile">
      <form ref={form} className="sc-settings-form" noValidate onSubmit={submit} aria-label="Profile">
        <div className="sc-settings-identity">
          <Avatar initials={initials || "?"} label={initials ? `Avatar: ${initials}` : "Avatar"} />
          <p>
            <strong>{displayName.trim() || "Your name"}</strong>
            <span>@{chosen ?? (handle || "handle")}</span>
          </p>
        </div>
        <TextField
          label="Display name"
          autoComplete="name"
          value={displayName}
          error={errors.displayName}
          onChange={(event) => {
            setDisplayName(event.target.value);
            clearError("displayName");
          }}
        />
        {chosen ? (
          <TextField label="Handle" value={`@${chosen}`} readOnly hint="Your handle is permanent and can’t be changed." />
        ) : (
          <TextField
            label="Handle"
            hint={`${HANDLE_RULE} You can’t change it later.`}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            value={handle}
            error={errors.handle}
            onChange={(event) => {
              setHandle(event.target.value);
              clearError("handle");
            }}
          />
        )}
        <div className="sc-settings-actions">
          <Button type="submit" busy={busy}>
            Save profile
          </Button>
        </div>
        <SectionStatus status={busy ? "Saving your profile…" : saved} problem={problem && settingsFailureCopy(problem)} />
      </form>
    </SettingsSection>
  );
}
