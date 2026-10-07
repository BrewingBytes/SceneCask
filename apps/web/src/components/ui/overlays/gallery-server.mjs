import { fileURLToPath } from "node:url";
// Shares R04's isolated workspace runner; only the fixture app and port differ.
import { runGallery } from "../basic/gallery-runner.mjs";
await runGallery(fileURLToPath(new URL("gallery-app", import.meta.url)), 3105);
