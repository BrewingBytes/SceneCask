"use client";
import { useState } from "react";
import { AppShell } from "../../shell";
import { Avatar, Button, Checkbox, RadioGroup } from "../basic";
import "../basic/gallery.css";
import { ConfirmDialog, Menu, Sheet, ToastViewport, useToastQueue } from ".";

// Fixture latency for busy states. Real outcomes come from feature code, never primitives.
const LATENCY_MS = 600;
const settle = (fail: boolean) =>
  new Promise<void>((resolve, reject) =>
    setTimeout(() => (fail ? reject(new Error("fixture")) : resolve()), LATENCY_MS),
  );

type Pending = "through" | "erase" | "sheet" | "remove" | null;

/** Isolated overlay examples. Never mount as an account or feature route. */
export function OverlayGallery() {
  const [failNext, setFailNext] = useState(false);
  const [started, setStarted] = useState(0);
  const [account, setAccount] = useState(false);
  const [rowMenu, setRowMenu] = useState(false);
  const [sheetMenu, setSheetMenu] = useState(false);
  const [saves, setSaves] = useState(0);
  const [spoilers, setSpoilers] = useState(false);
  const [dialog, setDialog] = useState<"through" | "erase" | null>(null);
  const [sheet, setSheet] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [status, setStatus] = useState("plan_to_watch");
  const [pending, setPending] = useState<Pending>(null);
  const [failed, setFailed] = useState<Pending>(null);
  const [announcement, setAnnouncement] = useState("");
  const { toasts, show, update, dismiss } = useToastQueue();

  // Mirrors a feature mutation: one attempt per activation, failure keeps the overlay open.
  async function run(kind: Exclude<Pending, null>, onSuccess: () => void) {
    setPending(kind);
    setFailed(null);
    setStarted((count) => count + 1);
    const fail = failNext;
    setFailNext(false);
    try {
      await settle(fail);
      onSuccess();
    } catch {
      setFailed(kind);
    } finally {
      setPending(null);
    }
  }

  const showSuccess = (message: string) => {
    const id = show({
      tone: "success",
      message,
      onUndo: () => {
        update(id, { busy: "undo" });
        settle(false).then(() => {
          dismiss(id);
          show({ tone: "info", message: "Undone." });
        });
      },
    });
  };
  const showError = () => {
    const id = show({
      tone: "error",
      message: "We couldn’t mark S2 E8 watched. Nothing changed.",
      onRetry: () => {
        update(id, { busy: "retry" });
        settle(false).then(() => {
          dismiss(id);
          showSuccess("Marked S2 E8 watched");
        });
      },
    });
  };
  const errorFor = (kind: Pending) =>
    failed === kind ? "We couldn’t save that. Nothing changed." : null;
  const closeDialog = (open: boolean) => {
    if (!open) {
      setDialog(null);
      setFailed(null);
    }
  };

  return (
    <AppShell
      currentPath="/"
      betaEnabled
      unreadCount={0}
      accountAction={
        <Menu
          open={account}
          onOpenChange={setAccount}
          label="Account"
          trigger={<Avatar initials="AR" label="Ana Reyes" />}
          header={
            <>
              <strong>Ana Reyes</strong>
              <br />
              <span className="sc-hint">ana@example.test</span>
            </>
          }
          items={[
            { id: "settings", label: "Settings", href: "#settings" },
            {
              id: "notifications",
              label: "Notifications",
              onSelect: () => setAnnouncement("Notifications selected."),
            },
            {
              id: "spoilers",
              label: "Hide spoilers",
              checked: !spoilers,
              onSelect: () => setSpoilers((value) => !value),
            },
            { id: "export", label: "Export data", disabled: true },
            {
              id: "feedback",
              label: "Send feedback",
              onSelect: () => setAnnouncement("Send feedback selected."),
            },
            {
              id: "sign-out",
              label: "Sign out",
              onSelect: () => setAnnouncement("Sign out selected."),
            },
          ]}
        />
      }
    >
      <div className="sc-gallery-intro">
        <p className="sc-kicker">SceneCask / Overlay gallery</p>
        <h1>Sheets, dialogs, menus and toasts.</h1>
        <p className="sc-reading">
          Isolated fixtures with simulated outcomes. No API is called.
        </p>
      </div>
      <section className="sc-gallery-section" aria-labelledby="fixture-heading">
        <h2 id="fixture-heading">Fixture controls</h2>
        <div className="sc-gallery-card">
          <Checkbox
            label="Make the next action fail"
            checked={failNext}
            onChange={(event) => setFailNext(event.target.checked)}
          />
          <p role="status">{announcement}</p>
          <p data-testid="started">Actions started: {started}</p>
        </div>
      </section>
      <section className="sc-gallery-section" aria-labelledby="overlay-heading">
        <h2 id="overlay-heading">Overlays</h2>
        <div className="sc-gallery-grid">
          <div className="sc-gallery-card">
            <h3>Dialogs</h3>
            <div className="sc-actions">
              <Button onClick={() => setDialog("through")}>
                Mark watched through S2 E7…
              </Button>
              <Button variant="destructive" onClick={() => setDialog("erase")}>
                Erase history…
              </Button>
              <Button variant="secondary" onClick={() => setSheet(true)}>
                Add to library
              </Button>
              <Button
                variant="quiet"
                onClick={() => {
                  // Parent and nested layer open in the same commit.
                  setSheet(true);
                  setRemoveOpen(true);
                }}
              >
                Open nested confirmation
              </Button>
            </div>
          </div>
          <div className="sc-gallery-card">
            <h3>Episode row</h3>
            <div className="sc-actions">
              <span className="sc-code">S2 E8</span>
              <Menu
                open={rowMenu}
                onOpenChange={setRowMenu}
                label="More actions for S2 E8"
                trigger={<span aria-hidden="true">⋯</span>}
                align="start"
                items={[
                  {
                    id: "through",
                    label: "Mark watched through S2 E8…",
                    onSelect: () => setDialog("through"),
                  },
                  { id: "view", label: "View episode", href: "#episode" },
                  { id: "discuss", label: "Discuss", disabled: true },
                  {
                    id: "unwatch",
                    label: "Mark unwatched",
                    tone: "destructive",
                    onSelect: () => setAnnouncement("Mark unwatched selected."),
                  },
                ]}
              />
            </div>
          </div>
          <div className="sc-gallery-card">
            <h3>Toasts</h3>
            <div className="sc-actions">
              <Button
                variant="secondary"
                onClick={() => showSuccess("Marked S2 E7 watched")}
              >
                Show success toast
              </Button>
              <Button variant="secondary" onClick={showError}>
                Show error toast
              </Button>
              <Button
                variant="quiet"
                onClick={() => {
                  // Re-issuing one id with new copy restarts its 6s.
                  setSaves((count) => count + 1);
                  show({ id: "save", tone: "success", message: `Saved ${saves + 1}` });
                }}
              >
                Repeat save toast
              </Button>
            </div>
          </div>
        </div>
      </section>

      <ConfirmDialog
        open={dialog === "through"}
        onOpenChange={closeDialog}
        kicker="Hollow Orchard"
        title="Mark 4 episodes watched?"
        description="S2 E4–S2 E7 will be marked watched. Earlier watched, future and special episodes stay as they are."
        confirmLabel={errorFor("through") ? "Retry" : "Mark 4 watched"}
        busy={pending === "through"}
        error={errorFor("through")}
        onConfirm={() =>
          run("through", () => {
            setDialog(null);
            showSuccess("Marked 4 episodes watched");
          })
        }
      />
      <ConfirmDialog
        open={dialog === "erase"}
        onOpenChange={closeDialog}
        title="Erase watch history?"
        description="Every watched episode of Hollow Orchard will be marked unwatched. The show stays in your library."
        confirmLabel={errorFor("erase") ? "Retry" : "Erase history"}
        destructive
        busy={pending === "erase"}
        error={errorFor("erase")}
        onConfirm={() =>
          run("erase", () => {
            setDialog(null);
            showSuccess("History erased");
          })
        }
      />
      <Sheet
        open={sheet}
        onOpenChange={(open) => {
          setSheet(open);
          if (!open) setFailed(null);
        }}
        kicker="Add to library"
        title="Hollow Orchard"
        busy={pending === "sheet"}
        footer={
          <>
          <Button
            busy={pending === "sheet"}
            onClick={() =>
              run("sheet", () => {
                setSheet(false);
                showSuccess("Saved to Plan to watch");
              })
            }
          >
            {pending === "sheet" ? "Saving…" : errorFor("sheet") ? "Retry" : "Save"}
          </Button>
          <Menu
            open={sheetMenu}
            onOpenChange={setSheetMenu}
            label="More library actions"
            trigger={<span aria-hidden="true">⋯</span>}
            triggerVariant="secondary"
            items={[
              {
                id: "remove",
                label: "Remove from library…",
                onSelect: () => setRemoveOpen(true),
              },
            ]}
          />
          </>
        }
      >
        <RadioGroup
          label="Status"
          name="overlay-status"
          value={status}
          onChange={setStatus}
          options={[
            { value: "plan_to_watch", label: "Plan to watch" },
            { value: "watching", label: "Watching" },
            { value: "on_hold", label: "On hold" },
            { value: "dropped", label: "Dropped" },
          ]}
        />
        <p className="sc-reading">
          Progress stays separate: choose episodes on the show page, or use
          catch-up for released regular episodes.
        </p>
        <div role="alert" className="sc-modal-error">
          {errorFor("sheet")}
        </div>
        <Button variant="destructive" onClick={() => setRemoveOpen(true)}>
          Remove from library…
        </Button>
        <ConfirmDialog
          open={removeOpen}
          onOpenChange={(open) => {
            setRemoveOpen(open);
            if (!open) setFailed(null);
          }}
          title="Remove Hollow Orchard?"
          description="Your watch history is kept. You can add the show again at any time."
          confirmLabel={errorFor("remove") ? "Retry" : "Remove"}
          destructive
          busy={pending === "remove"}
          error={errorFor("remove")}
          onConfirm={() =>
            run("remove", () => {
              setRemoveOpen(false);
              setSheet(false);
              showSuccess("Removed from library");
            })
          }
        />
      </Sheet>
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </AppShell>
  );
}
