import { expect, test } from "@playwright/test";
test("web and real API share an origin", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("A place for your TV story.");
  for (const [path, status] of [["/health/live", "live"], ["/health/ready", "ready"]]) {
    const response = await request.get(path);
    expect(response.status()).toBe(200);
    expect(await response.json()).toEqual({ status });
  }
});
for (const width of [320, 390, 859, 860, 1440]) {
  test(`foundation layout at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(await page.locator("main").evaluate(el => getComputedStyle(el).paddingLeft)).toBe(width >= 860 ? "48px" : "20px");
    if (width === 390 || width === 1440) await page.screenshot({ path: `test-results/foundation-${width}.png`, fullPage: true });
  });
}
