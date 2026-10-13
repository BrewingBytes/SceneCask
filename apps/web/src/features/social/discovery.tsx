"use client";
import Link from "next/link";
import { useState } from "react";
import { Avatar, EmptyState } from "../../components/ui/basic";
import { episodeCode } from "../tracking/copy";
import { createSocialApi, type ActivityEvent, type Recommendation } from "./api";
import { savedBy, when } from "./copy";
import { PagedList } from "./paged-list";
import { ShowEntry } from "./show-entry";
import { usePaged } from "./use-paged";
import "./social.css";

const findPeople = (
  <Link className="sc-button sc-button-secondary" href="/friends">
    Find people
  </Link>
);

/**
 * D02 Home "From people you follow" (beta): `GET /activity`, from approved follows only as the
 * API rechecks them at read. Show titles and episode codes only, never episode titles, stills or
 * comments. With nobody to follow it says so plainly; Home stays complete without it.
 */
export function FriendsActivity() {
  const [api] = useState(createSocialApi);
  const { list, retry, more } = usePaged(api.activity, (event: ActivityEvent) => event.id);
  return (
    <section className="sc-social-section sc-social-feed" aria-labelledby="sc-social-activity">
      <h2 id="sc-social-activity">From people you follow</h2>
      <PagedList
        list={list}
        label="Recent activity"
        loadingLabel="Loading activity…"
        errorTitle="Activity didn’t load"
        empty={
          <EmptyState title="Nothing from people you follow yet" action={findPeople}>
            Follow people to see what they’re watching. Your tracking works the same without them.
          </EmptyState>
        }
        retry={retry}
        more={() => void more()}
        itemKey={(event) => event.id}
      >
        {(event) => <ActivityRow event={event} />}
      </PagedList>
    </section>
  );
}

function ActivityRow({ event }: { event: ActivityEvent }) {
  const time = when(event.createdAt);
  return (
    <div className="sc-social-activity">
      <span aria-hidden="true">
        <Avatar initials={event.actor.initials || "?"} label={event.actor.displayName} />
      </span>
      <div className="sc-social-activity-main">
        <p className="sc-social-activity-text">
          <Link href={`/people/${event.actor.id}`} className="sc-social-person-name">
            {event.actor.displayName}
          </Link>{" "}
          {event.kind === "library_added" ? "saved" : "watched"}{" "}
          <Link href={`/shows/${event.show.id}`}>
            <em>{event.show.title}</em>
          </Link>
          {event.episode && (
            <>
              {" "}
              <span className="sc-code">{episodeCode(event.episode.season, event.episode.number)}</span>
            </>
          )}
        </p>
        <time className="sc-social-time" dateTime={event.createdAt} title={time.full}>
          {time.short}
        </time>
      </div>
    </div>
  );
}

/**
 * Discover "Saved by people you follow" (beta): `GET /recommendations`, aggregated by the API
 * from approved follows' visible libraries in its own order. No friends: a search prompt, never a
 * fabricated feed.
 */
export function FriendRecommendations() {
  const [api] = useState(createSocialApi);
  const { list, retry, more } = usePaged(api.recommendations, (item: Recommendation) => item.show.id);
  return (
    <section className="sc-social-section" aria-labelledby="sc-social-recommendations">
      <h2 id="sc-social-recommendations">Saved by people you follow</h2>
      <PagedList
        list={list}
        label="Shows saved by people you follow"
        loadingLabel="Loading suggestions…"
        errorTitle="Suggestions didn’t load"
        empty={
          <EmptyState title="No suggestions from friends yet" action={findPeople}>
            Shows saved by people you follow appear here when you haven’t saved them yourself. Search works without them.
          </EmptyState>
        }
        retry={retry}
        more={() => void more()}
        itemKey={(item) => item.show.id}
        className="sc-social-list sc-social-shows"
      >
        {(item) => (
          <ShowEntry show={item.show}>
            <span className="sc-social-person-detail">{savedBy(item.reason.people)}</span>
          </ShowEntry>
        )}
      </PagedList>
    </section>
  );
}
