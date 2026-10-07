import "../../../classes";
export { classes } from "../../../classes";
export async function loadMessage() {
  return (await import("./copy-message")).message;
}
