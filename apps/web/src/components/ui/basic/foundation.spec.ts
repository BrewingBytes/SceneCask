import axe from "axe-core";
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
    page.getByRole("link", { name: "Home", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(
    page.getByRole("link", { name: "Home", exact: true }),
  ).toHaveAttribute("href", "/");
  await expect(
    page.getByRole("link", { name: "SceneCask home" }),
  ).toHaveAttribute("href", "/");
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
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await page.evaluate(() => document.fonts.ready);
    await page.addScriptTag({ content: axe.source });
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

test("SSR image failure before hydration is recovered", async ({ page }) => {
  let releaseScripts!: () => void;
  const blocked = new Promise<void>((resolve) => {
    releaseScripts = resolve;
  });
  await page.route(/\.js(?:\?|$)/, async (route) => {
    await blocked;
    await route.continue();
  });
  try {
    await page.goto("/pre-hydration", { waitUntil: "commit" });
    const image = page.locator(".sc-art img");
    await expect(image).toHaveCount(1);
    await expect
      .poll(() =>
        image.evaluate(
          (el) =>
            (el as HTMLImageElement).complete &&
            (el as HTMLImageElement).naturalWidth === 0,
        ),
      )
      .toBe(true);
    const geometry = await page.locator(".sc-art").boundingBox();
    releaseScripts();
    await expect(image).toHaveCount(0);
    await expect(page.getByRole("img", { name: "SSR artwork" })).toBeVisible();
    expect(await page.locator(".sc-art").boundingBox()).toEqual(geometry);
  } finally {
    releaseScripts();
  }
});

test("empty/stale/disabled selection keeps an enabled tab and matching panel reachable", async ({
  page,
}) => {
  await page.goto("/regressions");
  for (const trigger of [null, "Use stale tab", "Disable selected tab"]) {
    if (trigger) await page.getByRole("button", { name: trigger }).click();
    const first = page.getByRole("tab", { name: "First", exact: true });
    await expect(first).toHaveAttribute("tabindex", "0");
    await expect(first).toHaveAttribute("aria-selected", "true");
    await expect(page.getByRole("tabpanel")).toHaveText("First panel");
    await page.getByRole("button", { name: "Disable selected tab" }).focus();
    await page.keyboard.press("Tab");
    await expect(first).toBeFocused();
  }
});

test("busy button retains keyboard focus and blocks repeated clicks, Enter, Space and submission", async ({
  page,
}) => {
  await page.goto("/regressions");
  const retry = page.getByRole("button", {
    name: "Retry fixture",
    exact: true,
  });
  await retry.focus();
  await page.keyboard.press("Enter");
  await expect(retry).toBeFocused();
  await expect(retry).not.toHaveAttribute("disabled");
  await expect(retry).toHaveAttribute("aria-disabled", "true");
  const submits = await page.getByLabel("Submit count").textContent();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Space");
  await retry.dispatchEvent("click");
  await expect(page.getByLabel("Attempt count")).toHaveText("1");
  await expect(page.getByLabel("Submit count")).toHaveText(submits!);
  await expect(retry).toBeFocused();
});

test("Unicode initials and shared field descriptions", async ({ page }) => {
  await page.goto("/regressions");
  await expect(page.getByRole("img", { name: "Unicode initials" })).toHaveText(
    "AB😀",
  );
  await expect(page.getByRole("img", { name: "Flag initials" })).toHaveText(
    "🇷🇴🇺🇸",
  );
  await expect(page.getByRole("img", { name: "Family initials" })).toHaveText(
    "👨‍👩‍👧‍👦AB",
  );
  await expect(page.getByLabel("Shared input")).toHaveAccessibleDescription(
    "Additional instructions. Input hint. Input error.",
  );
  await expect(page.getByLabel("Shared textarea")).toHaveAccessibleDescription(
    "Additional instructions. Textarea hint. Textarea error.",
  );
  await expect(page.getByLabel("Shared textarea")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
});

test("shell navigation uses a client transition", async ({ page }) => {
  await page.goto("/regressions");
  await page.evaluate(() => {
    (window as unknown as { navigationMarker: string }).navigationMarker =
      "preserved";
  });
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Home", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "A place for your TV story." }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        (window as unknown as { navigationMarker: string }).navigationMarker,
    ),
  ).toBe("preserved");
});

test("rendered dimensions and shape respond to CSS tokens", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Save show", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "Saved example." }),
  ).toBeVisible();
  await page.evaluate(() => {
    const style = document.documentElement.style;
    style.setProperty("--sc-radius-sm", "18px");
    style.setProperty("--sc-layout-touch-target-min", "60px");
    style.setProperty("--sc-layout-gutter-0", "28px");
  });
  await page.setViewportSize({ width: 390, height: 900 });
  const button = page.getByRole("button", { name: "Save show", exact: true });
  expect(await button.evaluate((el) => getComputedStyle(el).borderRadius)).toBe(
    "18px",
  );
  expect((await button.boundingBox())!.height).toBeGreaterThanOrEqual(60);
  expect(
    await page
      .locator("main")
      .evaluate((el) => getComputedStyle(el).paddingLeft),
  ).toBe("28px");
});

test("valid zero intrinsic width SVG survives hydration and unrelated renders", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const descriptor = Object.getOwnPropertyDescriptor(
      HTMLImageElement.prototype,
      "naturalWidth",
    )!;
    Object.defineProperty(HTMLImageElement.prototype, "naturalWidth", {
      get() {
        return this.src.startsWith("data:image/svg+xml,")
          ? 0
          : descriptor.get!.call(this);
      },
    });
    const decode = HTMLImageElement.prototype.decode;
    HTMLImageElement.prototype.decode = function () {
      this.dataset.decodeCalls = String(
        Number(this.dataset.decodeCalls ?? 0) + 1,
      );
      return decode.call(this);
    };
  });
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(/\.js(?:\?|$)/, async (route) => {
    await gate;
    await route.continue();
  });
  try {
    await page.goto("/svg-hydration", { waitUntil: "commit" });
    const image = page.getByRole("img", { name: "Valid SVG" });
    await expect
      .poll(() => image.evaluate((el) => (el as HTMLImageElement).complete))
      .toBe(true);
    release();
    await expect(image).toHaveAttribute("data-decode-calls", /^[1-9]\d*$/);
    await page.getByRole("button", { name: "Render 0" }).click();
    await expect(page.getByRole("button", { name: "Render 1" })).toBeVisible();
    await expect(image).toBeVisible();
    // Development StrictMode reattaches refs on mount; compare subsequent renders.
    const calls = await image.getAttribute("data-decode-calls");
    await page.getByRole("button", { name: "Render 1" }).click();
    await expect(page.getByRole("button", { name: "Render 2" })).toBeVisible();
    await expect(image).toHaveAttribute("data-decode-calls", calls!);
  } finally {
    release();
  }
});

test("static primitives render and native choices work without JavaScript", async ({
  browser,
}) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const page = await context.newPage();
    const response = await page.goto("/server-primitives");
    expect(response?.status()).toBe(200);
    await expect(page.getByText("Server badge")).toBeVisible();
    await expect(page.getByRole("img", { name: "Server avatar" })).toHaveText(
      "🇷🇴🇺🇸",
    );
    await expect(
      page.getByRole("progressbar", { name: "Server progress" }),
    ).toHaveAttribute("aria-valuenow", "25");
    await expect(page.getByRole("status")).toHaveText("Server loading");
    await expect(
      page.getByRole("heading", { name: "Server empty" }),
    ).toBeVisible();
    await page.getByLabel("Native checkbox").check();
    await expect(page.getByLabel("Native checkbox")).toBeChecked();
    await page.getByLabel("Second native choice").check();
    await expect(page.getByLabel("Second native choice")).toBeChecked();
    await expect(page.getByLabel("First native choice")).not.toBeChecked();
  } finally {
    await context.close();
  }
});
