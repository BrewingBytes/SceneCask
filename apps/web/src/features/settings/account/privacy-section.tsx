"use client";
import Link from "next/link";
import { useState } from "react";
import { Button, RadioGroup } from "../../../components/ui/basic";
import { ConfirmDialog } from "../../../components/ui/overlays";
import { endExpiredSession } from "../../auth";
import type { Failure, SettingsApi, Visibility } from "./api";
import { PUBLIC_WARNING, settingsFailureCopy } from "./copy";
import { SectionStatus, SettingsSection } from "./section";

const OPTIONS = [
  { value: "private", label: "Private: only followers you approve see your library and activity" },
  { value: "public", label: "Public: any signed-in member can see your library and activity" },
] as const;

export interface PrivacySectionProps {
  api: SettingsApi;
  visibility: Visibility;
  onChanged: (visibility: Visibility) => void;
}

/**
 * D04 Privacy (beta). Going public is confirmed before anything is written. The radios show
 * the server's visibility, plus the choice being saved while the request is in flight; a failed
 * save returns them to the server's value.
 */
export function PrivacySection({ api, visibility, onChanged }: PrivacySectionProps) {
  const [confirming, setConfirming] = useState(false);
  const [saving, setSaving] = useState<Visibility | null>(null);
  const [status, setStatus] = useState("");
  const [failed, setFailed] = useState<{ wanted: Visibility; failure: Failure } | null>(null);

  async function save(wanted: Visibility) {
    setConfirming(false);
    setSaving(wanted);
    setFailed(null);
    setStatus("");
    const result = await api.setVisibility(wanted);
    setSaving(null);
    if (result.ok) {
      onChanged(result.data.visibility);
      return setStatus(
        result.data.visibility === "public"
          ? "Your account is now public."
          : "Your account is now private. Followers you’ve approved keep access.",
      );
    }
    if (result.failure.kind === "auth") return endExpiredSession();
    setFailed({ wanted, failure: result.failure });
  }

  /** Ignored while saving. The radios stay enabled so focus can return to them from the warning. */
  function choose(value: string) {
    const wanted = value as Visibility;
    if (wanted === visibility || saving) return;
    if (wanted === "public") setConfirming(true);
    else void save(wanted);
  }

  const shown = saving ?? (confirming ? "public" : visibility);
  return (
    <SettingsSection
      id="privacy"
      title="Privacy"
      lede={
        <>
          Discussions always need mutual follows, whichever you choose. Pending follow requests stay pending until you
          answer them in <Link href="/friends">Friends</Link>.
        </>
      }
    >
      <div className="sc-settings-privacy" aria-busy={saving ? true : undefined}>
        <RadioGroup
          label="Who can see your library and activity"
          name="visibility"
          value={shown}
          onChange={choose}
          options={OPTIONS}
        />
      </div>
      <SectionStatus
        status={saving ? "Saving your privacy setting…" : status}
        problem={
          failed && (
            <div className="sc-settings-inline-error">
              <p>
                Your account is still {visibility}. {settingsFailureCopy(failed.failure)}
              </p>
              <Button variant="secondary" onClick={() => (failed.wanted === "public" ? setConfirming(true) : void save(failed.wanted))}>
                Try again
              </Button>
            </div>
          )
        }
      />
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title="Make your account public?"
        confirmLabel="Make public"
        onConfirm={() => void save("public")}
      >
        <p>{PUBLIC_WARNING}</p>
        <p>People who already asked to follow you stay pending until you answer.</p>
      </ConfirmDialog>
    </SettingsSection>
  );
}
