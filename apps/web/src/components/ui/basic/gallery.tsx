"use client";
import { useState } from "react";
import { AppShell } from "../../shell";
import {
  Avatar,
  Badge,
  Button,
  Checkbox,
  EmptyState,
  ErrorState,
  Poster,
  Progress,
  RadioGroup,
  Skeleton,
  Tabs,
  TextArea,
  TextField,
} from ".";

/** Isolated component examples. Never mount as an account or feature route. */
export function ComponentGallery({
  betaEnabled = false,
}: {
  betaEnabled?: boolean;
}) {
  const [tab, setTab] = useState("watching");
  const [visibility, setVisibility] = useState("private");
  const [announcement, setAnnouncement] = useState("");
  const [errorVisible, setErrorVisible] = useState(true);
  return (
    <AppShell
      currentPath="/library"
      betaEnabled={betaEnabled}
      unreadCount={3}
      accountAction={
        <Button variant="quiet" aria-label="Account">
          <Avatar initials="SC" label="Account initials" />
        </Button>
      }
    >
      <div className="sc-gallery-intro">
        <p className="sc-kicker">SceneCask / Component gallery</p>
        <h1>
          A place for your
          <br />
          TV story.
        </h1>
        <p className="sc-reading">
          Warm paper, clear choices, and room for the next episode.
        </p>
        <p>Design foundation · isolated examples</p>
      </div>
      <section
        className="sc-gallery-section"
        aria-labelledby="controls-heading"
      >
        <h2 id="controls-heading">Controls</h2>
        <div className="sc-gallery-grid">
          <div className="sc-gallery-card">
            <h3>Buttons & labels</h3>
            <div className="sc-actions">
              <Button onClick={() => setAnnouncement("Saved example.")}>
                Save show
              </Button>
              <Button variant="secondary">Secondary</Button>
              <Button variant="quiet">Quiet</Button>
              <Button variant="destructive">Remove</Button>
              <Button disabled>Unavailable</Button>
              <Button busy>Saving</Button>
            </div>
            <p className="sc-actions">
              <Badge>Watching</Badge>
              <Badge tone="accent">Caught up</Badge>
              <Badge tone="error">Needs attention</Badge>
            </p>
            <p role="status">{announcement}</p>
            <Avatar initials="AN" label="Ana initials" />
          </div>
          <div className="sc-gallery-card sc-gallery-fields">
            <h3>Fields</h3>
            <TextField
              label="Display name"
              hint="Use the name people know you by."
              autoComplete="off"
            />
            <TextField
              label="Handle"
              defaultValue="a"
              error="Use 3–30 lowercase letters, numbers or underscores."
            />
            <TextField label="Unavailable field" disabled value="Read only" />
            <TextArea label="Notes" hint="Plain text." />
            <Checkbox label="Keep this preference" />
            <RadioGroup
              label="Visibility"
              name="gallery-visibility"
              value={visibility}
              onChange={setVisibility}
              options={[
                { value: "private", label: "Private" },
                { value: "public", label: "Public" },
              ]}
            />
          </div>
        </div>
      </section>
      <section
        className="sc-gallery-section"
        aria-labelledby="progress-heading"
      >
        <h2 id="progress-heading">Progress & artwork</h2>
        <div className="sc-gallery-grid">
          <div className="sc-gallery-card">
            <Tabs
              label="Library status examples"
              value={tab}
              onChange={setTab}
              tabs={[
                {
                  value: "watching",
                  label: "Watching",
                  content: (
                    <>
                      <p className="sc-code">2 of 6 episodes</p>
                      <Progress value={2} max={6} label="Episodes watched" />
                    </>
                  ),
                },
                {
                  value: "planned",
                  label: "Plan to watch",
                  content: <p>No episodes watched yet.</p>,
                },
                {
                  value: "unavailable",
                  label: "Unavailable",
                  disabled: true,
                  content: null,
                },
              ]}
            />
            <p className="sc-kicker">Loading</p>
            <Skeleton label="Loading show" height={24} />
            <br />
            <Skeleton label="Loading progress" width="65%" height={16} />
          </div>
          <div className="sc-gallery-card">
            <h3>Missing artwork</h3>
            <div className="sc-gallery-art">
              <Poster alt="Poster unavailable" />
              <Poster aspect="still" alt="Episode image unavailable" />
            </div>
          </div>
        </div>
      </section>
      <section className="sc-gallery-section" aria-labelledby="states-heading">
        <h2 id="states-heading">Empty & error states</h2>
        <div className="sc-gallery-grid">
          <EmptyState
            title="Your story starts here"
            action={
              <Button
                onClick={() => setAnnouncement("Discover example selected.")}
              >
                Find a show
              </Button>
            }
          >
            Save a show to start your personal library.
          </EmptyState>
          {errorVisible && (
            <ErrorState
              title="We couldn’t load your shows"
              onRetry={() => setAnnouncement("Retry example selected.")}
              onDismiss={() => setErrorVisible(false)}
            >
              Please try again. Your saved progress is safe.
            </ErrorState>
          )}
        </div>
      </section>
    </AppShell>
  );
}
