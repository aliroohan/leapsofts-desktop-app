import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateOutcome =
  | { kind: "none" }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Update check failed";
}

/** Check GitHub Releases, download + install if available. Errors are returned, never thrown. */
export async function checkAndInstallUpdate(): Promise<UpdateOutcome> {
  try {
    const update = await check();
    if (!update) {
      return { kind: "none" };
    }
    await update.downloadAndInstall();
    return { kind: "ready", version: update.version };
  } catch (err) {
    return { kind: "error", message: errorMessage(err) };
  }
}

export async function restartApp(): Promise<void> {
  await relaunch();
}
