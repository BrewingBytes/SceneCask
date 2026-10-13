import { fileURLToPath } from "node:url";
// Shares R04's isolated workspace runner; only the fixture app and port differ.
import { runGallery } from "../../components/ui/basic/gallery-runner.mjs";
// The production /friends and /people/[id] routes read the beta flag per request.
process.env.SCENECASK_BETA = "true";
await runGallery(fileURLToPath(new URL("fixture-app", import.meta.url)), 3128);
