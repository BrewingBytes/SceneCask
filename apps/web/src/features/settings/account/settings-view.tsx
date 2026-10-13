"use client";
import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";
import { ErrorState, Skeleton } from "../../../components/ui/basic";
import { endExpiredSession } from "../../auth";
import { createSettingsApi, type Failure, type Identities, type User } from "./api";
import { settingsFailureCopy, type GoogleReturn } from "./copy";
import { PeopleList } from "./people-list";
import { PrivacySection } from "./privacy-section";
import { ProfileSection } from "./profile-section";
import { SettingsRow, SettingsSection } from "./section";
import { SignInSection } from "./sign-in-section";
import "./settings.css";

export interface SettingsViewProps {
  /** SCENECASK_BETA: Privacy, Followers, Blocked people and Your data render only when on. */
  beta: boolean;
}

type Phase =
  | { name: "loading" }
  | { name: "failed"; failure: Failure }
  | { name: "ready"; user: User; identities: Identities; returned: GoogleReturned };

type GoogleReturned = { action: GoogleReturn; error: string | null } | null;

/** Reads and scrubs `?google=link|unlink[&error=code]`, left by a Google flow started here. */
function takeGoogleReturn(): GoogleReturned {
  const url = new URL(window.location.href);
  const action = url.searchParams.get("google");
  const error = url.searchParams.get("error");
  if (action !== "link" && action !== "unlink") return null;
  url.searchParams.delete("google");
  url.searchParams.delete("error");
  window.history.replaceState(window.history.state, "", url.pathname + url.search);
  return { action, error };
}

/**
 * D04 /settings: Profile, Sign-in methods, Privacy, Followers, Blocked people, Your data and
 * About in one 760px column. Everything shown comes from the API; nothing is a local toggle.
 */
export function SettingsView({ beta }: SettingsViewProps) {
  const [api] = useState(createSettingsApi);
  const [phase, setPhase] = useState<Phase>({ name: "loading" });
  const [attempt, setAttempt] = useState(0);
  // Taken once: a Retry after a failed load must not lose (or replay) the Google outcome.
  const returned = useRef<GoogleReturned | undefined>(undefined);

  useEffect(() => {
    let live = true;
    void Promise.all([api.session(), api.identities()]).then(([session, identities]) => {
      if (!live) return;
      if (session.ok && !session.data.user) return endExpiredSession();
      const failed = !session.ok ? session : !identities.ok ? identities : null;
      if (failed) {
        if (failed.failure.kind === "auth") return endExpiredSession();
        return setPhase({ name: "failed", failure: failed.failure });
      }
      if (session.ok && session.data.user && identities.ok) {
        if (returned.current === undefined) returned.current = takeGoogleReturn();
        setPhase({ name: "ready", user: session.data.user, identities: identities.data, returned: returned.current });
      }
    });
    return () => {
      live = false;
    };
  }, [api, attempt]);

  const update = useCallback(
    (change: Partial<Pick<Extract<Phase, { name: "ready" }>, "user" | "identities">>) =>
      setPhase((current) => (current.name === "ready" ? { ...current, ...change } : current)),
    [],
  );
  const loadFollowers = useCallback((cursor?: string) => api.followers(cursor), [api]);
  const loadBlocks = useCallback((cursor?: string) => api.blocks(cursor), [api]);

  return (
    <div className="sc-settings sc-reading-column">
      <h1 className="sc-settings-title">Settings</h1>
      {phase.name === "loading" && (
        <div className="sc-settings-loading">
          <Skeleton label="Loading your settings…" height={24} width="40%" />
          <Skeleton decorative height={120} />
          <Skeleton decorative height={120} />
        </div>
      )}
      {phase.name === "failed" && (
        <ErrorState
          title="Your settings didn’t load"
          onRetry={() => {
            setPhase({ name: "loading" });
            setAttempt((count) => count + 1);
          }}
        >
          {settingsFailureCopy(phase.failure)}
        </ErrorState>
      )}
      {phase.name === "ready" && (
        <>
          <ProfileSection api={api} user={phase.user} onSaved={(user) => update({ user })} />
          <SignInSection
            api={api}
            user={phase.user}
            identities={phase.identities}
            onIdentities={(identities) => update({ identities })}
            returned={phase.returned}
          />
          {beta && (
            <>
              <PrivacySection
                api={api}
                visibility={phase.user.visibility}
                onChanged={(visibility) => update({ user: { ...phase.user, visibility } })}
              />
              <PeopleList
                id="followers"
                title="Followers"
                lede="People who follow you. Removing someone doesn’t notify them."
                load={loadFollowers}
                remove={api.removeFollower}
                loadingLabel="Loading your followers…"
                empty={{ title: "No followers yet", body: "When people follow you, they appear here." }}
                loadError="Your followers didn’t load"
                actionLabel="Remove"
                confirm={(person) => ({
                  title: `Remove ${person.displayName} as a follower?`,
                  body: (
                    <p>
                      @{person.handle} loses access to your library and activity right away while your account is private.
                      They won’t be notified, and they can ask to follow you again.
                    </p>
                  ),
                  label: "Remove follower",
                })}
                done={(person) => `${person.displayName} no longer follows you.`}
              />
              <PeopleList
                id="blocked"
                title="Blocked people"
                lede="Blocked people can’t find you or see your activity, and you won’t see theirs."
                load={loadBlocks}
                remove={api.unblock}
                loadingLabel="Loading blocked people…"
                empty={{ title: "You haven’t blocked anyone", body: "People you block from their profile appear here." }}
                loadError="Blocked people didn’t load"
                actionLabel="Unblock"
                confirm={(person) => ({
                  title: `Unblock ${person.displayName}?`,
                  body: (
                    <p>
                      @{person.handle} will be able to find you and ask to follow you again. Follows between you were
                      removed when you blocked them and won’t come back.
                    </p>
                  ),
                  label: "Unblock",
                })}
                done={(person) => `${person.displayName} is unblocked. Previous follows weren’t restored.`}
              />
              <SettingsSection id="data" title="Your data">
                <p className="sc-settings-lede">
                  Download a copy of your SceneCask data, or delete your account.{" "}
                  <Link href="/settings/data">Manage your data</Link>
                </p>
              </SettingsSection>
            </>
          )}
        </>
      )}
      <SettingsSection id="about" title="About">
        <p className="sc-settings-lede">This product uses the TMDB API but is not endorsed or certified by TMDB.</p>
        <SettingsRow
          title="TV metadata by TMDB"
          detail="Listings can be incomplete or change. Your watch history is kept when they do."
          action={
            <>
              <a className="sc-button sc-button-secondary" href="https://www.themoviedb.org/" rel="noreferrer">
                TMDB website
              </a>
              <Link className="sc-button sc-button-quiet" href="/about">
                About SceneCask
              </Link>
            </>
          }
        />
      </SettingsSection>
    </div>
  );
}
