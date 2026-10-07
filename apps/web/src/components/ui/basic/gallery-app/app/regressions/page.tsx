"use client";
import { Navigation } from "../../../../../shell/navigation";
import { useState } from "react";
import { Avatar, Button, Tabs, TextArea, TextField } from "../../../index";
export default function RegressionFixture() {
  const [value, setValue] = useState("");
  const [disabled, setDisabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [attempts, setAttempts] = useState(0);
  const [submits, setSubmits] = useState(0);
  return (
    <main className="sc-foundation" style={{ padding: 24 }}>
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
      <Tabs
        label="Resilient tabs"
        value={value}
        onChange={setValue}
        tabs={[
          { value: "first", label: "First", content: "First panel" },
          {
            value: "second",
            label: "Second",
            content: "Second panel",
            disabled,
          },
        ]}
      />
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setSubmits((count) => count + 1);
        }}
      >
        <Button
          type="submit"
          busy={busy}
          onClick={() => {
            setBusy(true);
            setAttempts((count) => count + 1);
          }}
        >
          Retry fixture
        </Button>
        <output aria-label="Attempt count">{attempts}</output>
        <output aria-label="Submit count">{submits}</output>
      </form>
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
  );
}
