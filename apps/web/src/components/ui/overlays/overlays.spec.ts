import { readFileSync } from "node:fs";
import { join } from "node:path";
import axe from "axe-core";
import { expect, test, type Page } from "@playwright/test";

const activeInside = (page: Page, selector: string) =>
  page.evaluate(
    (selector) => !!document.activeElement?.closest(selector),
    selector,
  );

async function tabTo(page: Page, name: string) {
  for (let step = 0; step < 60; step++) {
    await page.keyboard.press("Tab");
    if (
      await page.evaluate(
        (name) => document.activeElement?.textContent?.trim() === name,
        name,
      )
    )
      return;
  }
  throw new Error(`Could not reach ${name} by keyboard`);
}

test("keyboard-opened dialog traps focus and restores the trigger on cancel", async ({
  page,
}) => {
  await page.goto("/");
  await tabTo(page, "Mark watched through S2 E7…");
  const trigger = page.getByRole("button", {
    name: "Mark watched through S2 E7…",
  });
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("alertdialog", {
    name: "Mark 4 episodes watched?",
  });
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute("aria-modal", "true");
  await expect(dialog).toHaveAccessibleDescription(/S2 E4–S2 E7/);
  // The least destructive action receives initial focus.
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  for (const key of ["Tab", "Tab", "Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab"]) {
    await page.keyboard.press(key);
    expect(await activeInside(page, "[role=alertdialog]")).toBe(true);
  }
  // Background content is inert, so it cannot be clicked or focused.
  expect(
    await page.evaluate(() =>
      Array.from(document.body.children)
        .filter((child) => !child.classList.contains("sc-overlay"))
        .filter((child) => !child.hasAttribute("data-sc-overlay-persist"))
        .every((child) => (child as HTMLElement).inert || child.tagName === "SCRIPT"),
    ),
  ).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(
    await page.evaluate(() => document.querySelectorAll("[inert]").length),
  ).toBe(0);

  await page.keyboard.press("Enter");
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Enter"); // Cancel has focus
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect(page.getByTestId("started")).toHaveText("Actions started: 0");
});

test("sheet closes by Escape, scrim and close button, restoring focus and scroll", async ({
  page,
}) => {
  await page.goto("/");
  const trigger = page.getByRole("button", { name: "Add to library" });
  const sheet = page.getByRole("dialog", { name: "Hollow Orchard" });
  await trigger.focus();
  await page.keyboard.press("Enter");
  await expect(sheet).toBeVisible();
  expect(await page.evaluate(() => document.body.style.overflow)).toBe(
    "hidden",
  );
  // Initial focus lands in the body, on the selected status radio.
  await expect(page.getByRole("radio", { name: "Plan to watch" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(await page.evaluate(() => document.body.style.overflow)).toBe("");

  await trigger.click();
  await page.mouse.click(5, 5);
  await expect(sheet).toHaveCount(0);
  await expect(trigger).toBeFocused();

  await trigger.click();
  await sheet.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test("nested confirmation closes only the top layer", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Add to library" }).click();
  const sheet = page.getByRole("dialog", { name: "Hollow Orchard" });
  const remove = sheet.getByRole("button", { name: "Remove from library…" });
  await remove.click();
  const confirm = page.getByRole("alertdialog", {
    name: "Remove Hollow Orchard?",
  });
  await expect(confirm.getByRole("button", { name: "Cancel" })).toBeFocused();
  for (let step = 0; step < 4; step++) {
    await page.keyboard.press("Tab");
    expect(await activeInside(page, "[role=alertdialog]")).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expect(confirm).toHaveCount(0);
  await expect(sheet).toBeVisible();
  await expect(remove).toBeFocused();
  expect(await page.evaluate(() => document.body.style.overflow)).toBe(
    "hidden",
  );
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Add to library" })).toBeFocused();
});

test("parent and nested layers opened together stack in nesting order", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open nested confirmation" }).click();
  const confirm = page.getByRole("alertdialog", {
    name: "Remove Hollow Orchard?",
  });
  await expect(confirm.getByRole("button", { name: "Cancel" })).toBeFocused();
  expect(await confirm.evaluate((el) => !!el.closest("[inert]"))).toBe(false);
  await page.keyboard.press("Escape");
  await expect(confirm).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "Hollow Orchard" })).toBeVisible();
});

test("portals appended while a modal is open become inert", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Add to library" }).click();
  const late = await page.evaluate(async () => {
    const element = document.createElement("div");
    document.body.append(element);
    await new Promise((resolve) => setTimeout(resolve));
    return element.inert;
  });
  expect(late).toBe(true);
  await page.keyboard.press("Escape");
  expect(
    await page.evaluate(() => document.querySelectorAll("[inert]").length),
  ).toBe(0);
});

test("Tab in a menu inside a sheet returns to its trigger", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Add to library" }).click();
  const trigger = page.getByRole("button", { name: "More library actions" });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const item = page.getByRole("menuitem", { name: "Remove from library…" });
  await expect(item).toBeFocused();
  // Opens upward inside the panel, unclipped.
  const panel = (await page.locator(".sc-modal").boundingBox())!;
  const menu = (await page.getByRole("menu").boundingBox())!;
  expect(menu.y).toBeGreaterThanOrEqual(panel.y);
  await page.keyboard.press("Tab");
  await expect(trigger).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await expect(page.getByRole("dialog", { name: "Hollow Orchard" })).toBeVisible();
});

test("busy confirmation blocks duplicates and dismissal; failure persists with Retry", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Make the next action fail" }).check();
  const trigger = page.getByRole("button", { name: "Erase history…" });
  await trigger.click();
  const dialog = page.getByRole("alertdialog", { name: "Erase watch history?" });
  const confirm = dialog.getByRole("button", { name: "Erase history" });
  await confirm.click();
  await expect(confirm).toHaveAttribute("aria-busy", "true");
  await confirm.click({ force: true });
  await page.keyboard.press("Enter");
  await page.keyboard.press("Escape");
  await dialog.getByRole("button", { name: "Cancel" }).click({ force: true });
  await page.mouse.click(5, 5);
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("started")).toHaveText("Actions started: 1");

  // Failure keeps the dialog open with a persistent alert and Retry.
  await expect(dialog.getByRole("alert")).toHaveText(
    "We couldn’t save that. Nothing changed.",
  );
  const retry = dialog.getByRole("button", { name: "Retry" });
  await expect(retry).toBeEnabled();
  await page.waitForTimeout(1000);
  await expect(dialog.getByRole("alert")).toHaveText(/Nothing changed/);
  await retry.click();
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect(page.getByTestId("started")).toHaveText("Actions started: 2");
  await expect(
    page.getByRole("status").filter({ hasText: "History erased" }),
  ).toBeVisible();
});

test("success toast expires after 6s; error toast persists with Retry and Dismiss", async ({
  page,
}) => {
  await page.clock.install();
  await page.goto("/");
  const region = page.getByRole("region", { name: "Notifications" });
  const status = region.getByRole("status");
  const alert = region.getByRole("alert");
  // Live regions exist before any message so the first toast is announced.
  await expect(status).toBeAttached();
  await expect(alert).toBeAttached();

  await page.getByRole("button", { name: "Show success toast" }).click();
  await page.getByRole("button", { name: "Show error toast" }).click();
  await expect(status).toContainText("Marked S2 E7 watched");
  await expect(status.getByRole("button", { name: "Undo" })).toBeVisible();
  await expect(alert).toContainText("We couldn’t mark S2 E8 watched");
  await page.mouse.move(0, 0);
  await page.clock.runFor(5900);
  await expect(status).toContainText("Marked S2 E7 watched");
  await page.clock.runFor(200);
  await expect(status).not.toContainText("Marked S2 E7 watched");
  await page.clock.runFor(60_000);
  await expect(alert).toContainText("We couldn’t mark S2 E8 watched");
  await expect(alert.getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(alert.getByRole("button", { name: "Dismiss" })).toBeVisible();

  // Retry runs once while busy, then the caller replaces the error with success.
  const retry = alert.getByRole("button", { name: "Retry" });
  await retry.click();
  await expect(retry).toHaveAttribute("aria-busy", "true");
  await retry.click({ force: true });
  await page.clock.runFor(600);
  await expect(alert).toBeEmpty();
  await expect(status).toContainText("Marked S2 E8 watched");

  await page.getByRole("button", { name: "Show error toast" }).click();
  await alert.getByRole("button", { name: "Dismiss" }).click();
  await expect(alert).toBeEmpty();
});

test("re-issuing a toast id with new copy restarts its 6s", async ({ page }) => {
  await page.clock.install();
  await page.goto("/");
  const status = page.getByRole("region", { name: "Notifications" }).getByRole("status");
  const repeat = page.getByRole("button", { name: "Repeat save toast" });
  await repeat.click();
  await page.mouse.move(0, 0);
  await page.clock.runFor(5000);
  await repeat.focus();
  await page.keyboard.press("Enter");
  await expect(status).toHaveText(/Saved 2/);
  await expect(status.getByText(/Saved/)).toHaveCount(1);
  await page.clock.runFor(5000);
  await expect(status).toHaveText(/Saved 2/);
  await page.clock.runFor(1100);
  await expect(status).not.toHaveText(/Saved/);
});

test("focused or hovered toasts pause the timer", async ({ page }) => {
  await page.clock.install();
  await page.goto("/");
  const status = page.getByRole("region", { name: "Notifications" }).getByRole("status");
  await page.getByRole("button", { name: "Show success toast" }).click();
  await status.getByRole("button", { name: "Undo" }).focus();
  await page.clock.runFor(10_000);
  await expect(status).toContainText("Marked S2 E7 watched");
  await page.getByRole("button", { name: "Add to library" }).focus();
  await page.clock.runFor(6100);
  await expect(status).not.toContainText("Marked S2 E7 watched");
});

test("menu button keyboard, selection and outside dismissal", async ({ page }) => {
  await page.goto("/");
  const trigger = page.getByRole("button", { name: "Account" });
  await expect(trigger).toHaveAttribute("aria-haspopup", "menu");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await trigger.focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu", { name: "Account" });
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  const settings = menu.getByRole("menuitem", { name: "Settings" });
  const signOut = menu.getByRole("menuitem", { name: "Sign out" });
  await expect(settings).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await expect(signOut).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(settings).toBeFocused();
  await page.keyboard.press("End");
  await expect(signOut).toBeFocused();
  await page.keyboard.press("Home");
  await expect(settings).toBeFocused();
  // A longer prefix keeps searching from the current item.
  await page.keyboard.press("s");
  await page.keyboard.press("e");
  await expect(settings).toBeFocused();
  await page.waitForTimeout(600);
  await page.keyboard.press("e");
  const exportItem = menu.getByRole("menuitem", { name: "Export data" });
  await expect(exportItem).toBeFocused();
  await expect(exportItem).toHaveAttribute("aria-disabled", "true");
  await page.keyboard.press("Enter");
  await expect(menu).toBeVisible();
  await expect(
    menu.getByRole("menuitemcheckbox", { name: "Hide spoilers" }),
  ).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();

  await page.keyboard.press("ArrowUp");
  await expect(signOut).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect(page.getByText("Sign out selected.")).toBeVisible();

  await page.keyboard.press(" ");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press(" ");
  await trigger.click();
  await expect(
    menu.getByRole("menuitemcheckbox", { name: "Hide spoilers" }),
  ).toHaveAttribute("aria-checked", "false");
  await page.keyboard.press("Tab");
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();

  await trigger.click();
  await expect(menu).toBeVisible();
  await page.getByRole("heading", { level: 1 }).click();
  await expect(menu).toHaveCount(0);
});

test("menu item opens a dialog that returns focus to the menu trigger", async ({
  page,
}) => {
  await page.goto("/");
  const trigger = page.getByRole("button", { name: "More actions for S2 E8" });
  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("alertdialog", {
    name: "Mark 4 episodes watched?",
  });
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  // Escape inside a menu never closes the enclosing dialog layer.
  await page.getByRole("button", { name: "Add to library" }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("reduced motion disables overlay transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await page.getByRole("button", { name: "Show success toast" }).click();
  await page.getByRole("button", { name: "Add to library" }).click();
  for (const selector of [".sc-overlay", ".sc-modal", ".sc-toast"])
    expect(
      await page
        .locator(selector)
        .first()
        .evaluate((el) => getComputedStyle(el).animationName),
    ).toBe("none");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Account" }).click();
  expect(
    await page
      .locator(".sc-menu-popover")
      .evaluate((el) => getComputedStyle(el).animationName),
  ).toBe("none");

  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Add to library" }).click();
  expect(
    await page
      .locator(".sc-modal")
      .evaluate((el) => getComputedStyle(el).animationName),
  ).toBe("sc-overlay-rise");
});

test("overlay breakpoint matches the generated layout token", () => {
  const styles = join(__dirname, "../../../styles/tokens.css");
  const token = readFileSync(styles, "utf8").match(
    /--sc-layout-breakpoint-wide:\s*([^;]+);/,
  )![1];
  const css = readFileSync(join(__dirname, "overlays.css"), "utf8");
  expect([...css.matchAll(/@media \(min-width: ([^)]+)\)/g)].map((m) => m[1])).toEqual([token]);
});

async function openAll(page: Page, overlay: string) {
  if (overlay === "dialog")
    await page.getByRole("button", { name: "Mark watched through S2 E7…" }).click();
  if (overlay === "sheet")
    await page.getByRole("button", { name: "Add to library" }).click();
  if (overlay === "menu") await page.getByRole("button", { name: "Account" }).click();
  if (overlay === "toasts") {
    await page.getByRole("button", { name: "Show success toast" }).click();
    // The success toast may cover the fixture button on narrow screens.
    await page.getByRole("button", { name: "Show error toast" }).focus();
    await page.keyboard.press("Enter");
  }
}

async function closeAll(page: Page, overlay: string) {
  if (overlay !== "toasts") return page.keyboard.press("Escape");
  // Success may already have expired; errors always need an explicit Dismiss.
  const dismiss = page.getByRole("button", { name: "Dismiss" });
  while (await dismiss.count()) await dismiss.first().click();
}

for (const width of [320, 390, 859, 860, 1440])
  test(`responsive overlays at ${width}`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width, height: 800 });
    // Frozen timers keep the success toast on screen for measurement and screenshots.
    await page.clock.install();
    await page.goto("/");
    await page.clock.pauseAt(Date.now() + 60_000);
    await page.evaluate(() => document.fonts.ready);
    for (const overlay of ["dialog", "sheet", "menu", "toasts"]) {
      await openAll(page, overlay);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      const surface = page
        .locator(
          { dialog: ".sc-modal", sheet: ".sc-modal", menu: ".sc-menu-popover", toasts: ".sc-toast-viewport" }[overlay]!,
        );
      const box = (await surface.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(width);
      if (overlay === "dialog" || overlay === "sheet") {
        // Bottom-anchored below 860px, centered from 860px.
        if (width < 860) {
          expect(box.y + box.height).toBeCloseTo(800, 0);
          expect(box.width).toBeCloseTo(Math.min(width, overlay === "dialog" ? 480 : 560), 0);
        } else {
          expect(Math.abs(box.y + box.height / 2 - 400)).toBeLessThan(2);
          expect(box.width).toBe(overlay === "dialog" ? 480 : 560);
        }
      }
      if (overlay === "toasts") {
        // Clears the 72px mobile navigation; sits lower on desktop.
        expect(800 - (box.y + box.height)).toBe(width < 860 ? 88 : 32);
      }
      for (const control of await surface
        .locator("button, a, input")
        .all()) {
        if (!(await control.isVisible())) continue;
        if ((await control.getAttribute("type")) === "radio") continue;
        const target = (await control.boundingBox())!;
        expect(target.width).toBeGreaterThanOrEqual(44);
        expect(target.height).toBeGreaterThanOrEqual(44);
      }
      if ([320, 390, 1440].includes(width))
        await page.screenshot({
          path: `test-results/r05/${overlay}-${width}.png`,
        });
      await closeAll(page, overlay);
    }
  });

for (const width of [390, 1440])
  test(`axe accessibility with open overlays at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await page.evaluate(() => document.fonts.ready);
    await page.addScriptTag({ content: axe.source });
    const audit = () =>
      page.evaluate(async () => {
        const axe = (
          window as unknown as {
            axe: {
              run: (options: object) => Promise<{
                violations: { id: string; nodes: { target: string[] }[] }[];
              }>;
            };
          }
        ).axe;
        return (
          await axe.run({
            runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] },
          })
        ).violations.map(({ id, nodes }) => ({
          id,
          targets: nodes.map((node) => node.target.join(" ")),
        }));
      });
    for (const overlay of ["dialog", "sheet", "menu", "toasts"]) {
      await openAll(page, overlay);
      expect(await audit(), overlay).toEqual([]);
      await closeAll(page, overlay);
    }
    // Failure state inside a dialog.
    await page.getByRole("checkbox", { name: "Make the next action fail" }).check();
    await page.getByRole("button", { name: "Erase history…" }).click();
    await page.getByRole("button", { name: "Erase history", exact: true }).click();
    await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
    expect(await audit(), "dialog error").toEqual([]);
    await page.screenshot({ path: `test-results/r05/dialog-error-${width}.png` });
  });
