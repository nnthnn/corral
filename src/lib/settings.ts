import { DEFAULT_SETTINGS, useStore } from "../store";
import type { Settings } from "../types";
import { invoke } from "./tauri";
import { showError } from "./toasts";

export async function loadSettings(): Promise<void> {
  let settings: Settings;
  try {
    settings = await invoke("get_settings");
  } catch {
    settings = { ...DEFAULT_SETTINGS };
  }
  useStore.setState({
    settings,
    collapsedGroups: new Set(settings.collapsed_groups),
  });
}

export async function saveSettings(): Promise<void> {
  await invoke("save_settings", { settings: useStore.getState().settings });
}

let settingsSaveTimer: number | null = null;

export function scheduleSettingsSave(): void {
  if (settingsSaveTimer) clearTimeout(settingsSaveTimer);
  settingsSaveTimer = window.setTimeout(() => {
    settingsSaveTimer = null;
    saveSettings().catch((err) => showError(`Settings wouldn't save: ${err}`));
  }, 500);
}

/** Cancel a pending debounced save — called after a manual Save in the
 * settings form completes, so it doesn't also fire a redundant duplicate
 * save shortly after. */
export function cancelScheduledSettingsSave(): void {
  if (settingsSaveTimer) {
    clearTimeout(settingsSaveTimer);
    settingsSaveTimer = null;
  }
}

/** Patch a single settings field and debounce-persist it — the shared
 * implementation behind every quick UI toggle (sidebar/changes-pane
 * collapse, group collapse, etc.), so they share one persistence
 * strategy instead of each hand-rolling its own. */
export function toggleSetting<K extends keyof Settings>(
  key: K,
  value: Settings[K],
): void {
  const s = useStore.getState();
  useStore.setState({ settings: { ...s.settings, [key]: value } });
  scheduleSettingsSave();
}
