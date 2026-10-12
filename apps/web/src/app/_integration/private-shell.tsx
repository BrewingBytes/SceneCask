import type { ReactNode } from "react";
import { connection } from "next/server";
import { AppShell } from "../../components/shell";
import { AccountMenu } from "./account-menu";

/**
 * SCENECASK_BETA, read per request on the server (default off). The same variable gates the
 * API's social routes, so the UI never offers what the server would answer 404.
 */
export async function betaEnabled() {
  await connection();
  return process.env.SCENECASK_BETA === "true";
}

/** The signed-in application frame: Home is `/home`, plus the account menu and beta flag. */
export async function PrivateShell({ currentPath, children }: { currentPath: string; children: ReactNode }) {
  return (
    <AppShell currentPath={currentPath} homeHref="/home" betaEnabled={await betaEnabled()} accountAction={<AccountMenu />}>
      {children}
    </AppShell>
  );
}
