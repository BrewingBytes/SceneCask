import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

/** Translate the design handoff's dimensions/references into usable CSS values. */
export function generateDesignTokens(tokens) {
  const declarations = [];
  function flatten(value, path) {
    if (value == null) return;
    if (Array.isArray(value)) {
      value.forEach((item, index) => flatten(item, [...path, index]));
      return;
    }
    if (typeof value === "object") {
      Object.entries(value)
        .filter(([key]) => !key.startsWith("_"))
        .forEach(([key, item]) => flatten(item, [...path, key]));
      return;
    }
    let css = value;
    const key = path.join("-");
    if (typeof value === "number") {
      if (!Number.isFinite(value))
        throw new Error(`Invalid numeric design token: ${key}`);
      if (
        ["space", "layout", "radius"].includes(path[0]) ||
        path.includes("size") ||
        key === "focus-offset"
      )
        css = `${value}px`;
    } else if (typeof value === "string") {
      if (path[0] === "border" || key === "focus-ring") {
        css = value.replace(/\b([a-z][a-z-]*)$/, (name) => {
          if (!Object.hasOwn(tokens.color ?? {}, name))
            throw new Error(`Unknown color reference in design token: ${key}`);
          return `var(--sc-color-${name})`;
        });
      } else if (path.at(-1) === "family") {
        if (!Object.hasOwn(tokens.font ?? {}, value))
          throw new Error(`Unknown font reference in design token: ${key}`);
        css = `var(--sc-font-${value})`;
      } else if (key === "type-kicker-case")
        css = value === "upper" ? "uppercase" : value;
      else if (key === "motion-toast-auto-dismiss") {
        const duration = value.match(/^\d+(?:\.\d+)?(?:ms|s)\b/);
        if (!duration)
          throw new Error(`Invalid duration in design token: ${key}`);
        css = duration[0];
      } else if (key === "motion-reduced-motion") return; // Instructional prose, applied by the media rule.
    } else throw new Error(`Unsupported design token type: ${key}`);
    declarations.push(`  --sc-${key}: ${css};`);
  }
  flatten(tokens, []);
  return `/* Generated from approved handoff tokens. Run node apps/web/src/styles/generate-tokens.mjs. */\n:root {\n${declarations.join("\n")}\n}\n`;
}

export function generateFoundationStyles(tokens, template) {
  const breakpoint = tokens.layout?.["breakpoint-wide"];
  if (
    typeof breakpoint !== "number" ||
    !Number.isFinite(breakpoint) ||
    breakpoint <= 0
  )
    throw new Error("Invalid design token: layout-breakpoint-wide");
  // CSS custom properties cannot be substituted inside media query conditions.
  return `/* Generated from foundation.template.css and approved tokens. Do not edit directly. */\n${template.replaceAll("__SC_BREAKPOINT_WIDE__", `${breakpoint}px`)}`;
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const tokens = JSON.parse(
    readFileSync(
      new URL(
        "../../../../docs/rebuild/design/source/handoff/tokens.json",
        import.meta.url,
      ),
      "utf8",
    ),
  );
  const outputs = [
    ["tokens.css", generateDesignTokens(tokens)],
    [
      "foundation.css",
      generateFoundationStyles(
        tokens,
        readFileSync(
          new URL("./foundation.template.css", import.meta.url),
          "utf8",
        ),
      ),
    ],
  ];
  for (const [name, content] of outputs) {
    const file = new URL(`./${name}`, import.meta.url);
    if (process.argv.includes("--check")) {
      if (readFileSync(file, "utf8") !== content)
        throw new Error(`Generated design stylesheet has drifted: ${name}`);
    } else writeFileSync(file, content);
  }
}
