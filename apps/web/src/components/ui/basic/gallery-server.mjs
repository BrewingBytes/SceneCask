import { fileURLToPath } from "node:url";
import { runGallery } from "./gallery-runner.mjs";
await runGallery(fileURLToPath(new URL("gallery-app", import.meta.url)), 3104);
