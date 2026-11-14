import { listen } from "@tauri-apps/api/event";
import { initBrowserPreview } from "./lib/browser-events";
import { loadAll } from "./lib/data";
import { initNotifications } from "./lib/notifications";
import { initProvisioning } from "./lib/provisioning-events";
import { initRun } from "./lib/run-events";
import { loadSettings } from "./lib/settings";
import { invoke } from "./lib/tauri";
import { needsCustomChrome } from "./lib/window-chrome";
import { initWorktreeWatch } from "./lib/worktree-watch-events";
import { useStore } from "./store";

/** Everything `ui/app.ts`'s `initApp()` used to do that isn't rendering a
 * component: load persisted settings, request notification permission, wire
 * the backend's filesystem-watch event, and do the initial worktree load.
 * Called once from `main.tsx`, after the initial React render. */
export async function bootstrap(): Promise<void> {
  await loadSettings();
  initNotifications();
  invoke("home_dir")
    .then((homeDir) => useStore.setState({ homeDir: homeDir ?? undefined }))
    .catch(() => {
      /* path display just falls back to the full path */
    });
  needsCustomChrome().then((customChromeNeeded) =>
    useStore.setState({ needsCustomChrome: customChromeNeeded }),
  );
  // Backend watches ~/source for renames/creates/removes done outside the
  // GUI (e.g. an agent running `corral rename` in its own PTY) and emits
  // this whenever something changes, so the sidebar catches up without a
  // manual refresh.
  await listen("fs-changed", () => loadAll());
  await initWorktreeWatch();
  await initBrowserPreview();
  await initProvisioning();
  await initRun();
  await loadAll();
}
