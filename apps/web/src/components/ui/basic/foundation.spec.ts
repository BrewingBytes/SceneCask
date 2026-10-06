import { expect, test } from "@playwright/test";

for (const width of [320, 390, 859, 860, 1440]) {
  test(`responsive gallery at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    const nav = page.getByRole("navigation", { name: "Main navigation" });
    expect(await nav.evaluate((el) => getComputedStyle(el).position)).toBe(
      width >= 860 ? "static" : "fixed",
    );
    expect(
      await page
        .locator("main")
        .evaluate((el) => getComputedStyle(el).paddingLeft),
    ).toBe(width >= 860 ? "48px" : "20px");
    for (const control of await page
      .locator(".sc-foundation")
      .locator(
        "button, a:not(.sc-skip), .sc-choice, input:not([type=checkbox],[type=radio]), textarea",
      )
      .all()) {
      if (!(await control.isVisible())) continue;
      const box = await control.boundingBox();
      expect(box!.width).toBeGreaterThanOrEqual(44);
      expect(box!.height).toBeGreaterThanOrEqual(44);
    }
    await page.evaluate(() => document.fonts.ready);
    if (width === 390 || width === 1440)
      await page.screenshot({
        path: `test-results/r04/gallery-${width}.png`,
        fullPage: true,
      });
    await page.locator(".sc-error").scrollIntoViewIfNeeded();
    if (width === 390 || width === 1440)
      await page.screenshot({ path: `test-results/r04/states-${width}.png` });
  });
}

test("alpha navigation excludes beta access", async ({ page }) => {
  await page.goto("/alpha");
  await expect(page.getByRole("navigation").getByRole("link")).toHaveCount(3);
  await expect(page.getByRole("link", { name: /Notifications/ })).toHaveCount(
    0,
  );
  await page.goto("/");
  await expect(page.getByRole("navigation").getByRole("link")).toHaveCount(4);
  await expect(
    page.getByRole("link", { name: "Notifications, 3 unread" }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Library", exact: true }),
  ).toHaveAttribute("aria-current", "page");
});

test("keyboard, fields, disabled buttons and persistent errors", async ({
  page,
}) => {
  await page.goto("/");
  await page.keyboard.press("Tab");
  await expect(
    page.getByRole("link", { name: "Skip to content" }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("main")).toBeFocused();
  await expect(
    page.getByRole("button", { name: "Unavailable", exact: true }),
  ).toBeDisabled();
  await expect(page.getByRole("button", { name: /Saving/ })).toBeDisabled();
  await expect(page.getByLabel("Handle", { exact: true })).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  await expect(
    page.getByLabel("Handle", { exact: true }),
  ).toHaveAccessibleDescription(
    "Use 3–30 lowercase letters, numbers or underscores.",
  );
  await page.getByRole("tab", { name: "Watching", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Plan to watch" })).toBeFocused();
  await expect(page.getByRole("tabpanel")).toHaveText(
    "No episodes watched yet.",
  );
  await page.keyboard.press("ArrowRight");
  await expect(
    page.getByRole("tab", { name: "Watching", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("End");
  await expect(page.getByRole("tab", { name: "Plan to watch" })).toBeFocused();
  await page.getByLabel("Private", { exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByLabel("Public", { exact: true })).toBeChecked();
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.locator(".sc-error").getByRole("alert")).toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "Retry example selected." }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Dismiss", exact: true }).click();
  await expect(page.locator(".sc-error").getByRole("alert")).toHaveCount(0);
});

test("fallback ratios, local fonts and reduced motion", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (request) => {
    if (!request.url().startsWith("http://127.0.0.1:3104"))
      external.push(new URL(request.url()).host);
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  for (const [selector, ratio] of [
    [".sc-art-poster", 2 / 3],
    [".sc-art-still", 16 / 9],
  ] as const) {
    const box = await page.locator(selector).boundingBox();
    expect(box!.width / box!.height).toBeCloseTo(ratio, 2);
  }
  expect(
    await page
      .locator(".sc-skeleton")
      .first()
      .evaluate((el) => getComputedStyle(el).animationName),
  ).toBe("none");
  expect(
    await page
      .locator(".sc-progress span")
      .evaluate((el) => getComputedStyle(el).transitionDuration),
  ).toBe("0s");
  await page.evaluate(async () => {
    await Promise.all(
      ["Schibsted Grotesk", "Newsreader", "IBM Plex Mono"].map((font) =>
        document.fonts.load(`15px "${font}"`),
      ),
    );
  });
  expect(
    await page.evaluate(() =>
      ["Schibsted Grotesk", "Newsreader", "IBM Plex Mono"].every((font) =>
        document.fonts.check(`15px "${font}"`),
      ),
    ),
  ).toBe(true);
  expect(external).toEqual([]);
});

for (const width of [390, 1440]) {
  test(`axe accessibility and contrast at ${width}`, async ({ page }) => {
    const script = process.env.SCENECASK_AXE_SCRIPT;
    if (!script)
      throw new Error(
        "Set SCENECASK_AXE_SCRIPT to the pinned axe-core 4.10.3 axe.min.js file; see README.",
      );
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await page.evaluate(() => document.fonts.ready);
    await page.addScriptTag({ path: script });
    const violations = await page.evaluate(async () => {
      const axe = (
        window as unknown as {
          axe: {
            run: (options: object) => Promise<{
              violations: {
                id: string;
                nodes: { target: string[]; failureSummary: string }[];
              }[];
            }>;
          };
        }
      ).axe;
      return (
        await axe.run({
          runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] },
        })
      ).violations;
    });
    expect(violations).toEqual([]);
  });
}

test("failed artwork reserves geometry and a changed source can recover", async ({
  page,
}) => {
  await page.goto("/art");
  const art = page.locator(".sc-art");
  const before = await art.boundingBox();
  await page.getByRole("button", { name: "Load artwork" }).click();
  await expect(art.locator("img")).toBeVisible();
  expect(await art.boundingBox()).toEqual(before);
  await page.getByRole("button", { name: "Fail artwork" }).click();
  await expect(art.locator("img")).toHaveCount(0);
  await expect(art.getByRole("img", { name: "Fixture artwork" })).toBeVisible();
  expect(await art.boundingBox()).toEqual(before);
  await page.getByRole("button", { name: "Load artwork" }).click();
  await expect(art.locator("img")).toBeVisible();
  expect(await art.boundingBox()).toEqual(before);
});

for (const width of [390, 1440]) {
  test(`visible keyboard focus at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await page.getByRole("tab", { name: "Watching", exact: true }).focus();
    await page.keyboard.press("ArrowRight");
    const selected = page.getByRole("tab", { name: "Plan to watch" });
    await expect(selected).toBeFocused();
    expect(
      await selected.evaluate((el) => getComputedStyle(el).outlineWidth),
    ).toBe("3px");
    expect(
      await selected.evaluate((el) => getComputedStyle(el).outlineOffset),
    ).toBe("2px");
    await page.screenshot({ path: `test-results/r04/focus-${width}.png` });
  });
}
