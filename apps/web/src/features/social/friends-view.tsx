"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { ErrorState, Skeleton, Tabs } from "../../components/ui/basic";
import { createSocialApi, type Person } from "./api";
import { HANDLE_NEEDED, PARTICIPANTS_NOTE, parseFriendsTab, privacyLede, socialFailureCopy, type FriendsTab } from "./copy";
import { FindPeople } from "./find-people";
import { FollowersPanel } from "./followers-panel";
import { RequestsPanel } from "./requests-panel";
import { usePaged } from "./use-paged";
import { useViewer } from "./use-viewer";
import "./social.css";

export interface FriendsViewProps {
  /** From `?tab=`; a notification opens `requests`. */
  initialTab?: FriendsTab;
}

const personKey = (person: Person) => person.id;

/**
 * D08 /friends: Find people, Incoming requests and Followers. Everything shown comes from the
 * API; relationships change only when a write is accepted. The tab is kept in the URL.
 */
export function FriendsView({ initialTab = "find" }: FriendsViewProps) {
  const [api] = useState(createSocialApi);
  const { viewer, retry: retryViewer } = useViewer(api);
  const [tab, setTab] = useState<FriendsTab>(initialTab);
  const requests = usePaged(viewer.state === "ready" ? api.requests : null, personKey);
  const followers = usePaged(viewer.state === "ready" ? api.followers : null, personKey);
  const { update: updateRequests } = requests;
  const { update: updateFollowers, retry: reloadFollowers } = followers;

  const choose = useCallback((value: string) => {
    const next = parseFriendsTab(value);
    setTab(next);
    const url = new URL(window.location.href);
    if (next === "find") url.searchParams.delete("tab");
    else url.searchParams.set("tab", next);
    window.history.replaceState(window.history.state, "", url.pathname + url.search);
  }, []);

  const pending =
    requests.list.state === "ready" && requests.list.items.length > 0
      ? ` (${requests.list.items.length}${requests.list.nextCursor ? "+" : ""})`
      : "";

  return (
    <div className="sc-social sc-reading-column">
      <h1 className="sc-social-title">Friends</h1>
      {viewer.state === "loading" && (
        <div className="sc-social-loading">
          <Skeleton label="Loading Friends…" height={20} width="70%" />
          <Skeleton decorative height={56} />
        </div>
      )}
      {viewer.state === "error" && (
        <ErrorState title="Friends didn’t load" onRetry={retryViewer}>
          {socialFailureCopy(viewer.failure)}
        </ErrorState>
      )}
      {viewer.state === "ready" && (
        <>
          <p className="sc-social-lede">
            {privacyLede(viewer.user.visibility, viewer.user.handle)}{" "}
            <Link href="/settings#sc-settings-privacy">Privacy settings</Link>
          </p>
          <p className="sc-social-note">{PARTICIPANTS_NOTE}</p>
          {!viewer.user.handle && (
            <div className="sc-social-notice">
              <p>{HANDLE_NEEDED}</p>
              <Link className="sc-button sc-button-secondary" href="/onboarding/profile">
                Choose a handle
              </Link>
            </div>
          )}
          <Tabs
            label="Friends"
            value={tab}
            onChange={choose}
            tabs={[
              { value: "find", label: "Find people", content: <FindPeople api={api} /> },
              {
                value: "requests",
                label: `Incoming requests${pending}`,
                content: (
                  <RequestsPanel
                    api={api}
                    list={requests.list}
                    retry={requests.retry}
                    more={() => void requests.more()}
                    remove={(person) => updateRequests((items) => items.filter((item) => item.id !== person.id))}
                    onApproved={reloadFollowers}
                  />
                ),
              },
              {
                value: "followers",
                label: "Followers",
                content: (
                  <FollowersPanel
                    api={api}
                    list={followers.list}
                    retry={followers.retry}
                    more={() => void followers.more()}
                    remove={(person) => updateFollowers((items) => items.filter((item) => item.id !== person.id))}
                  />
                ),
              },
            ]}
          />
        </>
      )}
    </div>
  );
}
