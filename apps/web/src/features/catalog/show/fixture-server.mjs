import { fileURLToPath } from "node:url";
// Shares R04's isolated workspace runner; only the fixture app and port differ.
import { runGallery } from "../../../components/ui/basic/gallery-runner.mjs";
await runGallery(fileURLToPath(new URL("fixture-app", import.meta.url)), 3122);
