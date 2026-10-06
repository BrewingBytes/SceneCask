import { readFileSync, writeFileSync } from "node:fs";
const source = new URL(
  "../../../../docs/rebuild/design/source/handoff/tokens.json",
  import.meta.url,
);
const tokens = JSON.parse(readFileSync(source, "utf8"));
const declarations = [];
function flatten(value, path) {
  if (Array.isArray(value))
    value.forEach((item, index) => flatten(item, [...path, index]));
  else if (typeof value === "object")
    Object.entries(value)
      .filter(([key]) => !key.startsWith("_"))
      .forEach(([key, item]) => flatten(item, [...path, key]));
  else declarations.push(`  --sc-${path.join("-")}: ${value};`);
}
flatten(tokens, []);
const css = `/* Generated from approved handoff tokens. Run node apps/web/src/styles/generate-tokens.mjs. */\n:root {\n${declarations.join("\n")}\n}\n`;
const output = new URL("./tokens.css", import.meta.url);
if (process.argv.includes("--check")) {
  if (readFileSync(output, "utf8") !== css)
    throw new Error("Design token output has drifted.");
} else writeFileSync(output, css);
