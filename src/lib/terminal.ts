import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { type ITerminalOptions, Terminal } from "@xterm/xterm";
import { showError } from "./toasts";

/** Every `xterm.js` instance in this app (the interactive agent terminal in
 * `session.ts`, and the read-only log viewers `ProvisionPanel`/`RunPanel`/
 * `RunPanel`) needs the same two non-obvious options, or it either
 * crashes outright or silently breaks link clicks:
 * - `allowProposedApi: true` — `Unicode11Addon`'s `term.unicode.activeVersion`
 *   setter is a proposed API as of xterm v6 and throws without this.
 * - `linkHandler` — without it, real OSC 8 hyperlinks (as opposed to
 *   `WebLinksAddon`'s regex-matched plain-text URLs) fall back to xterm's
 *   built-in `confirm()`+`window.open()` handler, which Tauri's webview
 *   blocks outright ("dialog.confirm not allowed").
 * This one previously had to be fixed independently in three separate
 * files (and was missed in a fourth until it crashed) — centralized here
 * so a future xterm-level fix only needs to land once. */
export function createLinkedTerminal(options: ITerminalOptions): {
  term: Terminal;
  fit: FitAddon;
} {
  const term = new Terminal({
    ...options,
    allowProposedApi: true,
    linkHandler: {
      activate: (_e, uri) => void openUrl(uri),
    },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new WebLinksAddon((_e, uri) => void openUrl(uri)));
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";
  return { term, fit };
}

/** Attach Ctrl+C / Ctrl+Shift+C copy to a read-only log terminal (the Run and
 * Setup panels). These terminals have no interactive stdin, so Ctrl+C has no
 * SIGINT meaning — it always means "copy the current selection", via the same
 * clipboard plugin the agent terminal uses (whose own Ctrl+C path is a
 * selection-gated SIGINT passthrough that only makes sense on a live agent
 * PTY). Both chords hit the same branch because `e.key` is `"C"` when Shift
 * is held. Returning `false` stops xterm from also treating the key as
 * terminal input (a no-op on these read-only terminals) and suppresses the
 * native `copy` event for consistency with the agent terminal. */
export function attachCopyOnCtrlC(term: Terminal): void {
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown" || !e.ctrlKey || e.altKey || e.metaKey)
      return true;
    if (e.key.toLowerCase() === "c" && term.hasSelection()) {
      e.preventDefault();
      writeText(term.getSelection()).catch((err) =>
        showError(`Wouldn't copy: ${err}`),
      );
      return false;
    }
    return true;
  });
}
