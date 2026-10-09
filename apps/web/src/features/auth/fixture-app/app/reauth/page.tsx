"use client";
import { useState } from "react";
import { Button } from "../../../../../components/ui/basic";
import { ReauthDialog } from "../../../reauth-dialog";

/** Stand-in for a Settings action that needs fresh reauthentication (D04 owns the real one). */
export default function FixtureReauth() {
  const [open, setOpen] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  return (
    <main className="sc-foundation">
      <Button onClick={() => setOpen(true)}>Unlink Google</Button>
      <p role="status">{confirmed ? "Confirmed. Unlinking Google…" : ""}</p>
      <ReauthDialog
        open={open}
        onOpenChange={setOpen}
        onReauthenticated={() => setConfirmed(true)}
        passwordEnabled
        googleLinked
        returnTo="/settings"
      />
    </main>
  );
}
