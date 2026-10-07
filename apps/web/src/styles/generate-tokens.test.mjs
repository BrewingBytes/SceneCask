import { test } from "node:test";
import assert from "node:assert/strict";
import {
  generateDesignTokens,
  generateResponsiveStyles,
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
    generateResponsiveStyles(
      { layout: { "breakpoint-wide": 900 } },
      "@media (min-width: __SC_BREAKPOINT_WIDE__) {}",
    ),
    "/* Generated from foundation-responsive.template.css and approved tokens. Do not edit directly. */\n@media (min-width: 900px) {}",
  );
  assert.throws(
    () => generateResponsiveStyles({ layout: { "breakpoint-wide": null } }, ""),
    /layout-breakpoint-wide/,
  );
  assert.throws(
    () => generateDesignTokens({ border: { hairline: "1px solid absent" } }),
    /border-hairline/,
  );
});

test("border values ending in a hex color or CSS keyword pass through unchanged", () => {
  const css = generateDesignTokens({
    color: { line: "#ddd", "paper-2": "#eee" },
    border: {
      subtle: "1px solid paper-2",
      clear: "1px solid transparent",
      current: "2px dashed currentcolor",
      hex: "1px solid #fff",
    },
  });
  assert.match(css, /--sc-border-subtle: 1px solid var\(--sc-color-paper-2\);/);
  assert.match(css, /--sc-border-clear: 1px solid transparent;/);
  assert.match(css, /--sc-border-current: 2px dashed currentcolor;/);
  assert.match(css, /--sc-border-hex: 1px solid #fff;/);
});

test("new numeric tokens require explicit types instead of silently producing unitless CSS", () => {
  for (const tokens of [{ focus: { gap: 4 } }, { motion: { delay: 120 } }]) {
    assert.throws(
      () => generateDesignTokens(tokens),
      /explicit \$type\/\$value units/,
    );
  }
  const css = generateDesignTokens({
    focus: { gap: { $type: "dimension", $value: { value: 0.5, unit: "rem" } } },
    motion: {
      delay: { $type: "duration", $value: { value: 120, unit: "ms" } },
    },
  });
  assert.match(css, /--sc-focus-gap: 0.5rem;/);
  assert.match(css, /--sc-motion-delay: 120ms;/);
  assert.throws(
    () =>
      generateDesignTokens({
        focus: {
          gap: { $type: "dimension", $value: { value: 4, unit: "invalid" } },
        },
      }),
    /focus-gap/,
  );
});
