import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { type ITerminalOptions, Terminal } from "@xterm/xterm";
import { showError } from "./toasts";

/** xterm.js's font stack when the user hasn't picked a family in settings —
 * Linux-safe generic monospace first, with the usual macOS/macOS-ish names as
 * hints. */
export const DEFAULT_TERMINAL_FONT =
  'ui-monospace, "SF Mono", Menlo, monospace';

/** CSS generic font keywords — xterm hands `fontFamily` straight to CSS, so a
 * stack already ending in one of these needs no help. */
const GENERIC_FAMILIES = new Set([
  "monospace",
  "ui-monospace",
  "sans-serif",
  "ui-sans-serif",
  "serif",
  "ui-serif",
  "system-ui",
  "cursive",
  "fantasy",
  "math",
  "emoji",
  "fangsong",
  "ui-rounded",
]);

/** Whether `name` is a CSS generic font keyword rather than a concrete family
 * (see [`GENERIC_FAMILIES`]). Used both when resolving a stack's fallback and
 * when checking a configured family against the installed-font list. */
export function isGenericFontFamily(name: string): boolean {
  return GENERIC_FAMILIES.has(name.trim().toLowerCase());
}

/** Resolve the `terminal_font_family` setting to what xterm should be handed:
 * blank (the default) means [`DEFAULT_TERMINAL_FONT`]. A stack that doesn't
 * already end in a generic family gets [`DEFAULT_TERMINAL_FONT`] appended,
 * because a single *unavailable* family (e.g. a typo, or a datalist
 * suggestion that isn't installed) otherwise makes WebKit fall back to its
 * default **proportional** font — which breaks xterm's fixed cell grid rather
 * than just looking different. The full built-in stack (not just `monospace`)
 * keeps that degradation identical to the blank setting, e.g. landing on
 * `SF Mono` on macOS before reaching the generic. */
export function resolveTerminalFontFamily(family: string): string {
  const trimmed = family.trim();
  if (!trimmed) return DEFAULT_TERMINAL_FONT;
  const last = trimmed
    .split(",")
    .pop()
    ?.trim()
    .replace(/^["']|["']$/g, "")
    .toLowerCase();
  if (last && GENERIC_FAMILIES.has(last)) return trimmed;
  return `${trimmed}, ${DEFAULT_TERMINAL_FONT}`;
}

/** Apply a font setting change to an already-open terminal. Returns `true`
 * when the options actually changed, `false` when the terminal already had
 * them (e.g. it was just created with the current settings).
 *
 * xterm itself takes care of the hard parts on a `fontFamily`/`fontSize`
 * option change: `CharSizeService` re-measures the cell synchronously, and
 * the render service clears and fully repaints (rebuilt WebGL texture atlas
 * included), so no `refresh()`/`clearTextureAtlas()` call is needed here.
 * What xterm *can't* do is re-fit to the container or tell the PTY the new
 * rows/cols — that stays with each caller, since agent terminals and the
 * Run/Setup log panels resize through different commands. */
export function applyTerminalFont(
  term: Terminal,
  family: string,
  size: number,
): boolean {
  const applied = resolveTerminalFontFamily(family);
  if (term.options.fontFamily === applied && term.options.fontSize === size) {
    return false;
  }
  term.options.fontFamily = applied;
  term.options.fontSize = size;
  return true;
}

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
