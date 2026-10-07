import { test } from "node:test";
import assert from "node:assert/strict";
import {
  generateDesignTokens,
  generateFoundationStyles,
} from "./generate-tokens.mjs";

test("CSS tokens resolve references and remove annotations/nulls", () => {
  const css = generateDesignTokens({
    color: { line: "#ddd", accent: "#963" },
    font: { reading: "Georgia" },
    space: [4],
    optional: null,
    border: { hairline: "1px solid line" },
    focus: { ring: "3px solid accent", offset: 2 },
    type: { reading: { family: "reading" } },
    motion: {
      "toast-auto-dismiss": "6000ms (success only; errors persist)",
      "reduced-motion": "all transitions disabled",
    },
  });
  assert.match(css, /--sc-border-hairline: 1px solid var\(--sc-color-line\)/);
  assert.match(css, /--sc-focus-ring: 3px solid var\(--sc-color-accent\)/);
  assert.match(css, /--sc-type-reading-family: var\(--sc-font-reading\)/);
  assert.match(css, /--sc-space-0: 4px/);
  assert.match(css, /--sc-motion-toast-auto-dismiss: 6000ms;/);
  assert.doesNotMatch(css, /optional|success only|all transitions disabled/);
});
test("changed breakpoint updates generated media queries; invalid inputs identify the token", () => {
  assert.equal(
    generateFoundationStyles(
      { layout: { "breakpoint-wide": 900 } },
      "@media (min-width: __SC_BREAKPOINT_WIDE__) {}",
    ),
    "/* Generated from foundation.template.css and approved tokens. Do not edit directly. */\n@media (min-width: 900px) {}",
  );
  assert.throws(
    () => generateFoundationStyles({ layout: { "breakpoint-wide": null } }, ""),
    /layout-breakpoint-wide/,
  );
  assert.throws(
    () => generateDesignTokens({ border: { hairline: "1px solid absent" } }),
    /border-hairline/,
  );
});
