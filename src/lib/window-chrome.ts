import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Resolves `true` when corral should draw its own close button: either the
 * compositor/WM did not grant real decorations, or the check itself failed.
 * Failing toward "show the button" is deliberate — a redundant button next
 * to real OS decorations is a much smaller problem than a window with no
 * way to close it at all.
 */
export async function needsCustomChrome(): Promise<boolean> {
  try {
    const decorated = await getCurrentWindow().isDecorated();
    return !decorated;
  } catch {
    return true;
  }
}

export async function closeWindow(): Promise<void> {
  await getCurrentWindow().close();
}
