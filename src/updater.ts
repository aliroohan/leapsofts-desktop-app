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
    const message = errorMessage(err);
    const hint =
      message.includes("valid release JSON") || message.includes("ReleaseNotFound")
        ? " Could not load the update manifest (GitHub may be blocked or the release is still publishing). Try again in a few minutes or install the latest build from GitHub Releases."
        : "";
    return { kind: "error", message: message + hint };
  }
}

export async function restartApp(): Promise<void> {
  await relaunch();
}
