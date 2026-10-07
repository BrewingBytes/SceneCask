"use client";
import { Navigation } from "../../../../../shell/navigation";
import { classes, loadMessage } from "./copy-helper";
import { useEffect, useRef, useState } from "react";
import {
  Avatar,
  Button,
  Tabs,
  TextArea,
  TextField,
  RadioGroup,
  Poster,
  Skeleton,
} from "../../../index";
export default function RegressionFixture() {
  const [copyMessage, setCopyMessage] = useState("");
  const [value, setValue] = useState("");
  const [allDisabled, setAllDisabled] = useState(false);
  const [radio, setRadio] = useState("first");
  const [radioHandler, setRadioHandler] = useState(false);
  const [retryKey, setRetryKey] = useState(0);
  const [disabled, setDisabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [attempts, setAttempts] = useState(0);
  const [submits, setSubmits] = useState(0);
  const [ancestorClicks, setAncestorClicks] = useState(0);
  const [captures, setCaptures] = useState(0);
  const [keys, setKeys] = useState(0);
  const [isolatedSubmits, setIsolatedSubmits] = useState(0);
  const isolating = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Simulates a third-party wrapper that stops native click propagation.
    const wrapper = isolating.current;
    const stop = (event: Event) => event.stopPropagation();
    wrapper?.addEventListener("click", stop);
    return () => wrapper?.removeEventListener("click", stop);
  }, []);
  return (
    <>
      {/* Primitives must size correctly without the .sc-foundation wrapper. */}
      <div data-testid="unwrapped" style={{ width: 240, padding: 0 }}>
        <TextField label="Unwrapped input" />
      </div>
      <main className="sc-foundation" style={{ padding: 24 }}>
        <Button onClick={async () => setCopyMessage(await loadMessage())}>
          Load copied module
        </Button>
        <output aria-label="Copied module" className={classes("sc-code")}>
          {copyMessage}
        </output>
        <Navigation currentPath="/regressions" />
        <Button onClick={() => setValue("stale")}>Use stale tab</Button>
        <Button
          onClick={() => {
            setValue("second");
            setDisabled(true);
          }}
        >
          Disable selected tab
        </Button>
        <Button onClick={() => setAllDisabled(true)}>Disable all tabs</Button>
        <output aria-label="Selected tab">{value}</output>
        <Tabs
          label="Resilient tabs"
          value={value}
          onChange={setValue}
          tabs={[
            {
              value: "first",
              label: "First",
              content: "First panel",
              disabled: allDisabled,
            },
            {
              value: "second",
              label: "Second",
              content: "Second panel",
              disabled: disabled || allDisabled,
            },
          ]}
        />
        <Button onClick={() => setRadio("second")}>Change radio value</Button>
        <Button onClick={() => setRadioHandler(true)}>
          Enable radio callback
        </Button>
        <RadioGroup
          label="Changing radios"
          name="changing"
          value={radio}
          onChange={radioHandler ? setRadio : undefined}
          options={[
            { value: "first", label: "First controlled radio" },
            { value: "second", label: "Second controlled radio" },
          ]}
        />
        <Poster
          src="/recovering-art.svg"
          alt="Retry artwork"
          retryKey={retryKey}
        />
        <Button onClick={() => setRetryKey((key) => key + 1)}>
          Retry same artwork
        </Button>
        <form
          onClick={() => setAncestorClicks((count) => count + 1)}
          onSubmit={(event) => {
            event.preventDefault();
            setSubmits((count) => count + 1);
          }}
        >
          <Button
            type="submit"
            busy={busy}
            onClickCapture={() => setCaptures((count) => count + 1)}
            onKeyDown={() => setKeys((count) => count + 1)}
            onClick={() => {
              setBusy(true);
              setAttempts((count) => count + 1);
            }}
          >
            Retry fixture
          </Button>
          <output aria-label="Attempt count">{attempts}</output>
          <output aria-label="Submit count">{submits}</output>
          <output aria-label="Ancestor click count">{ancestorClicks}</output>
          <output aria-label="Capture count">{captures}</output>
          <output aria-label="Key count">{keys}</output>
        </form>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            setIsolatedSubmits((count) => count + 1);
          }}
        >
          <div ref={isolating}>
            <Button type="submit" busy>
              Isolated busy submit
            </Button>
          </div>
          <output aria-label="Wrapped form submissions">
            {isolatedSubmits}
          </output>
        </form>
        <TextField label="Plain input" hint="Plain hint." />
        <div data-testid="skeleton-list" aria-busy="true">
          <Skeleton label="Loading list" />
          <Skeleton decorative />
          <Skeleton decorative />
        </div>
        <Avatar initials="AB😀Z" label="Unicode initials" />
        <Avatar initials="🇷🇴🇺🇸" label="Flag initials" />
        <Avatar initials="👨‍👩‍👧‍👦ABC" label="Family initials" />
        <p id="extra-description">Additional instructions.</p>
        <TextField
          label="Shared input"
          hint="Input hint."
          error="Input error."
          aria-describedby="extra-description"
        />
        <TextArea
          label="Shared textarea"
          hint="Textarea hint."
          error="Textarea error."
          aria-describedby="extra-description"
        />
      </main>
    </>
  );
}
