import { fileURLToPath } from "node:url";
// Shares R04's isolated workspace runner; only the fixture app and port differ.
import { runGallery } from "../../../components/ui/basic/gallery-runner.mjs";
// The production /settings route reads the beta flag per request; /alpha/settings covers it off.
process.env.SCENECASK_BETA = "true";
await runGallery(fileURLToPath(new URL("fixture-app", import.meta.url)), 3125);
