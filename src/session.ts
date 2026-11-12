import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  readImage,
  readText,
  writeText,
} from "@tauri-apps/plugin-clipboard-manager";
import type { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import type { Terminal } from "@xterm/xterm";
import { decodeBase64 } from "./lib/base64";
import { detectOpencode2Hint } from "./lib/opencode2-hint";
import { invoke } from "./lib/tauri";
import { createLinkedTerminal } from "./lib/terminal";
import { showError } from "./lib/toasts";
import { useStore } from "./store";
import "@xterm/xterm/css/xterm.css";

export interface SessionInfo {
  id: string;
  project: string;
  feature: string;
  running: boolean;
  exit_code: number | null;
  killed: boolean;
  waiting: boolean;
  last_summary: string | null;
}

export interface Attach {
  info: SessionInfo;
  buffer: number[];
}

/** Whether the user's caret currently sits in a text-entry surface that is
 * NOT the terminal (sidebar note editor, command palette, a dialog field).
 * `show()` skips its focus grab while that's true, so a background session
 * event can't pull the caret away mid-sentence. `termsEl` is checked so
 * xterm's own hidden textarea (inside the terminal container) still counts as
 * terminal focus — switching worktrees must keep moving focus to the new
 * terminal. */
function isForeignTextEntryFocused(termsEl: HTMLElement): boolean {
  // Any open native dialog owns the keyboard, even when focus is momentarily
  // on its backdrop rather than a field.
  if (document.querySelector("dialog[open]")) return true;
  const el = document.activeElement;
  if (!el) return false;
  if (termsEl.contains(el)) return false;
  if (el instanceof HTMLInputElement) {
    // Buttons/checks/ranges aren't text entry — focusing the terminal after
    // clicking one of those is still the expected behavior.
    return ![
      "button",
      "submit",
      "reset",
      "checkbox",
      "radio",
      "range",
      "file",
      "color",
    ].includes(el.type);
  }
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
    return true;
  }
  return el instanceof HTMLElement && el.isContentEditable;
}

interface OutputEvent {
  id: string;
  /** Base64-encoded PTY bytes (see `base64_bytes` in the backend) — decoded
   * via [`decodeBase64`] before writing to the terminal. */
  data: string;
}

interface ExitEvent {
  id: string;
  code: number | null;
  killed: boolean;
  /** True when this was a `--resume` attempt and claude exited because the
   * resumed conversation no longer exists — retrying resume would just loop
   * forever, so this drives an automatic fresh-session fallback instead of
   * the manual "Restart" button. */
  resume_failed: boolean;
}

interface StatusEvent {
  id: string;
  waiting: boolean;
  last_summary: string | null;
}

// Mirrors DESIGN.md's bg/text tokens so the terminal blends into the pane
// instead of showing as a distinct cooler slate box. Cursor uses the warm
// `secondary` gold rather than `primary` red, since DESIGN.md reserves the
// red accent for exactly three spots (active sidebar row, active tab, the
// focused terminal pane's edge) and a blinking red cursor would read like a
// constant alert. The 16 ANSI colors are left at xterm's defaults — that's
// real tool output (git diff colors, ls colors), not chrome.
export const THEME = {
  background: "#1a1510",
  foreground: "#ede8e0",
  cursor: "#c49a3a",
  selectionBackground: "rgba(237, 232, 224, 0.16)",
};

const FONT = 'ui-monospace, "SF Mono", Menlo, monospace';

const DEFAULT_FONT_SIZE = 13;
const MIN_FONT_SIZE = 8;
const MAX_FONT_SIZE = 32;

/** How much recent decoded output to retain per session for opencode2 error
 * detection — comfortably larger than the longest known marker, so a marker
 * split across PTY chunks still matches. */
const OPENCODE2_HINT_BUFFER = 4096;

interface TermEntry {
  term: Terminal;
  fit: FitAddon;
  container: HTMLElement;
  /** Set when the WebGL renderer loaded successfully; used to clear its
   * texture atlas on a manual repaint. Undefined once contained fell back
   * to the default renderer. */
  webgl?: WebglAddon;
  /** Last size sent to `resize_session`, to skip redundant PTY resizes. */
  lastRows?: number;
  lastCols?: number;
  /** True once this entry has re-announced its size after first output —
   * see the `session-output` listener below. Some agent CLIs (observed with
   * opencode's `attach` TUI) read the terminal's starting dimensions once,
   * very early in their own startup, which can race ahead of the resize
   * `show()` already sends right after spawn — landing before the child's
   * resize-event listener is wired up and getting silently missed, leaving
   * the TUI rendered at the PTY's original 80x24 default. Re-announcing the
   * size once real output proves the child's event loop is alive closes
   * that race without needing a fixed delay. */
  resizeReannounced?: boolean;
  /** Inline "Restart" button shown after a non-deliberate exit; cleared on
   * new output or the next exit event for this session. */
  restartBtn?: HTMLButtonElement;
  /** True once this terminal's session has ended, so its cursor has been made
   * inert — solid rather than blinking, and hidden while the pane is
   * unfocused — instead of continuing to blink as if it were still awaiting
   * input. Cleared when fresh output proves a new session is live (see the
   * `session-output` listener). */
  cursorInert?: boolean;
}

/**
 * Holds one live xterm.js terminal per attached session inside `#terminals`.
 * Which one is visible is driven by the sidebar selection via [`show`]; hidden
 * terminals stay alive and keep receiving output, so switching is instant.
 */
export class SessionManager {
  private entries = new Map<string, TermEntry>();
  private activeId: string | null = null;
  private fontSize: number;
  private onFontSizeChange?: (size: number) => void;
  private fontFamily: string;
  private onFontFamilyChange?: (family: string) => void;
  private termsEl: HTMLElement;
  private onOutput?: (id: string) => void;
  private onExit?: (id: string, code: number | null, killed: boolean) => void;
  private onInput?: (id: string) => void;
  private onStatus?: (
    id: string,
    waiting: boolean,
    lastSummary: string | null,
  ) => void;
  private onRestart?: (id: string) => void;
  private onResumeFailed?: (id: string) => void;
  private onTerminalsChanged?: () => void;
  /** Rolling tail of each session's decoded output, kept just long enough to
   * catch a known error marker split across PTY chunks (see
   * [`checkOpencode2Hint`]). */
  private hintBuffers = new Map<string, string>();
  /** Sessions that have already surfaced their opencode2 hint — dedupes the
   * toast when a streaming error repeats. */
  private hintedSessions = new Set<string>();

  constructor(opts: {
    /** The `#terminals` element React mounts and owns; SessionManager
     * appends one child `.term-pane` per session inside it. */
    container: HTMLElement;
    /** Initial font size, e.g. loaded from persisted settings. */
    fontSize: number;
    /** Called whenever the font size changes (e.g. Ctrl/Cmd +/-/0), so the
     * caller can persist it — this class no longer owns persistence itself. */
    onFontSizeChange?: (size: number) => void;
    /** Initial font family; empty/undefined falls back to `FONT`. */
    fontFamily?: string;
    /** Called whenever the font family changes, with the raw (possibly
     * empty) setting value — so the caller can persist it verbatim. */
    onFontFamilyChange?: (family: string) => void;
    onOutput?: (id: string) => void;
    onExit?: (id: string, code: number | null, killed: boolean) => void;
    onInput?: (id: string) => void;
    onStatus?: (id: string, waiting: boolean) => void;
    onRestart?: (id: string) => void;
    onResumeFailed?: (id: string) => void;
    /** Called whenever a terminal entry is created or disposed, so a caller
     * relying on `has()` for render decisions (see `AgentPane`'s
     * `hasTerminal`) can force a re-render — mutating this class's private
     * `entries` map is otherwise invisible to React. */
    onTerminalsChanged?: () => void;
  }) {
    this.fontSize = Math.min(
      MAX_FONT_SIZE,
      Math.max(MIN_FONT_SIZE, opts.fontSize || DEFAULT_FONT_SIZE),
    );
    this.onFontSizeChange = opts.onFontSizeChange;
    this.fontFamily = opts.fontFamily?.trim() || FONT;
    this.onFontFamilyChange = opts.onFontFamilyChange;
    this.onOutput = opts.onOutput;
    this.onExit = opts.onExit;
    this.onInput = opts.onInput;
    this.onStatus = opts.onStatus;
    this.onRestart = opts.onRestart;
    this.onResumeFailed = opts.onResumeFailed;
    this.onTerminalsChanged = opts.onTerminalsChanged;
    this.termsEl = opts.container;

    // Refit the visible terminal whenever the pane's box actually changes, not
    // just on `window.resize`. Toggling the sidebar or Changes pane (and the
    // settings panel) resizes the grid columns without ever firing a window
    // resize, so the agent terminal used to keep its old width and leave dead
    // space — or overflow — until some unrelated resize fixed it. Same
    // self-healing pattern the Run/Setup log viewers use (`RunPanel.tsx`);
    // `fitActive()` itself no-ops while the pane is hidden (zero box), so this
    // can't shrink a live PTY to xterm's 1×1 minimum.
    new ResizeObserver(() => this.fitActive()).observe(this.termsEl);
    window.addEventListener("resize", () => this.fitActive());

    listen<OutputEvent>("session-output", (e) => {
      this.clearRestartOverlay(e.payload.id);
      const entry = this.entries.get(e.payload.id);
      const bytes = decodeBase64(e.payload.data);
      entry?.term.write(bytes);
      // Output means a (new) session is live again — restore the cursor to
      // its normal blinking state after a previous session made it inert.
      if (entry?.cursorInert) {
        entry.cursorInert = false;
        entry.term.options.cursorBlink = true;
        entry.term.options.cursorInactiveStyle = "outline";
      }
      this.checkOpencode2Hint(e.payload.id, bytes);
      // See `TermEntry.resizeReannounced`'s doc comment: re-send the size
      // once real output proves the child's own event loop is alive, to
      // close the race where its very first size read landed before our
      // post-spawn resize did. Only for the visible terminal — `fit()` on a
      // hidden (`display:none`) container would measure zero.
      if (entry && !entry.resizeReannounced && this.activeId === e.payload.id) {
        entry.resizeReannounced = true;
        this.reannounceSize(e.payload.id, entry);
      }
      this.onOutput?.(e.payload.id);
    });

    this.setupDragDrop();

    listen<ExitEvent>("session-exit", (e) => {
      // Update store state (marks the session not-running) before any
      // restart callback below, since restartFresh/restartAgent read that
      // state synchronously to guard against double-starting — calling
      // them first would see a stale `running: true` and silently no-op.
      this.onExit?.(e.payload.id, e.payload.code, e.payload.killed);

      const entry = this.entries.get(e.payload.id);
      if (entry) {
        const { code, killed, resume_failed } = e.payload;
        const msg = killed
          ? "[agent stopped]"
          : code != null
            ? `[agent exited (${code})]`
            : "[agent exited]";
        entry.term.write(`\r\n\x1b[90m${msg}\x1b[0m\r\n`);
        this.clearRestartOverlay(e.payload.id);
        // The PTY is gone, so nothing can be typed into it anymore: stop the
        // cursor blinking (and hide it entirely while the pane isn't focused)
        // so a dead terminal doesn't keep looking like it's waiting for
        // input. Restored on the next session's first output chunk.
        //
        // The agent's TUI almost certainly set its own cursor shape/blink with
        // DECSCUSR (`CSI Ps SP q`), and xterm lets that runtime override win
        // over `options.cursorBlink` — so the option change alone leaves the
        // cursor blinking. `CSI 0 SP q` clears that override, letting the
        // steady/none settings above actually apply. (The app re-sends its own
        // DECSCUSR on the next session's first output.)
        entry.cursorInert = true;
        entry.term.options.cursorBlink = false;
        entry.term.options.cursorInactiveStyle = "none";
        entry.term.write("\x1b[0 q");
        if (resume_failed) {
          entry.term.write(
            "\x1b[90m[resumed session is gone, starting a fresh one]\x1b[0m\r\n",
          );
          this.onResumeFailed?.(e.payload.id);
        } else if (!killed) {
          this.showRestartOverlay(e.payload.id, entry);
        }
      }
    });

    listen<StatusEvent>("session-status", (e) => {
      this.onStatus?.(e.payload.id, e.payload.waiting, e.payload.last_summary);
    });
  }

  /**
   * Real-terminal copy/paste conventions: Ctrl+C copies when text is
   * selected (otherwise it's SIGINT passthrough, which claude relies on to
   * interrupt a turn — so a selection is required to reinterpret it as
   * copy); Ctrl+V pastes from the system clipboard. Returning `false` stops
   * xterm from also treating the key as terminal input — but xterm's hidden
   * textarea *also* has its own native browser "paste" listener that fires
   * independently of this handler, so without `preventDefault()` the text
   * gets pasted a second time from that path. Same story for copy: without
   * it, the native "copy" event fires too (harmless here, but suppressed
   * for consistency).
   *
   * A clipboard image (e.g. a copied screenshot) has no text representation.
   * `arboard` (the underlying clipboard crate) rejects `get_text` outright in
   * that case rather than resolving empty, so `readText` throws — catch that
   * and fall back to `readImage`, save it to a temp file, and paste the
   * quoted path like a file drop, instead of surfacing a spurious error.
   */
  private handleKeyEvent(id: string, e: KeyboardEvent): boolean {
    if (e.type !== "keydown" || !e.ctrlKey || e.altKey || e.metaKey)
      return true;
    const entry = this.entries.get(id);
    if (!entry) return true;

    const key = e.key.toLowerCase();
    if (key === "c" && entry.term.hasSelection()) {
      e.preventDefault();
      writeText(entry.term.getSelection()).catch((err) =>
        showError(`Wouldn't copy: ${err}`),
      );
      return false;
    }
    if (key === "v") {
      e.preventDefault();
      readText()
        .catch(() => "")
        .then(async (text) => {
          if (text) {
            entry.term.paste(text);
            return;
          }
          const image = await readImage().catch(() => null);
          if (!image) return;
          const [rgba, size] = await Promise.all([image.rgba(), image.size()]);
          const png = await rgbaToPng(rgba, size.width, size.height);
          const path = await invoke("save_pasted_image", {
            bytes: Array.from(png),
          });
          entry.term.paste(`"${path}" `);
        })
        .catch((err) => showError(`Wouldn't paste: ${err}`));
      return false;
    }
    return true;
  }

  /**
   * OS-level file drops (real filesystem paths — unlike HTML5 drag events,
   * whose `File` objects don't expose a usable path in a webview) insert
   * quoted paths into the active terminal, the same as a Ctrl+V paste — the
   * PTY buffers it like any typed-ahead input if the agent is mid-turn.
   * (Previously gated on an "is the agent idle" heuristic, but that heuristic
   * lags up to 1.5s behind reality, which silently ate real drops far more
   * often than the mid-turn interleaving it was meant to prevent.)
   */
  private setupDragDrop() {
    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") {
        this.termsEl.classList.remove("drag-over");
        this.handleDrop(event.payload.paths, event.payload.position);
      } else if (event.payload.type === "over") {
        this.termsEl.classList.toggle(
          "drag-over",
          this.overTerminal(event.payload.position),
        );
      } else {
        this.termsEl.classList.remove("drag-over");
      }
    });
  }

  private overTerminal(position: { x: number; y: number }): boolean {
    const scale = window.devicePixelRatio || 1;
    const x = position.x / scale;
    const y = position.y / scale;
    const rect = this.termsEl.getBoundingClientRect();
    return (
      x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
    );
  }

  private handleDrop(paths: string[], position: { x: number; y: number }) {
    if (!paths.length || !this.activeId || !this.overTerminal(position)) return;
    const entry = this.entries.get(this.activeId);
    if (!entry) return;
    entry.term.paste(`${paths.map((p) => `"${p}"`).join(" ")} `);
    entry.term.focus();
  }

  has(id: string): boolean {
    return this.entries.has(id);
  }

  /** Call once `start_session` resolves for `id` — every session start spawns
   * a brand-new backend PTY (hardcoded back to the 24x80 default, regardless
   * of whatever size a previous session on this same worktree ended up at),
   * but `lastRows`/`lastCols`/`resizeReannounced` all live on the reused
   * xterm `TermEntry` and would otherwise still reflect the *previous*,
   * now-dead PTY's size. Without this, `fitActive` sees "no change" and
   * skips resending, and the first-output reannounce (already spent on the
   * previous session) never fires again — so the fresh PTY never learns its
   * real size and opencode's TUI renders at the hardcoded default. */
  resetSizeCache(id: string) {
    const entry = this.entries.get(id);
    if (!entry) return;
    entry.lastRows = undefined;
    entry.lastCols = undefined;
    entry.resizeReannounced = false;
  }

  /** Make `id`'s terminal the visible one, creating + attaching if needed. */
  async show(id: string) {
    if (!this.entries.has(id)) {
      await this.createTerminal(id);
    }
    this.activeId = id;
    for (const [eid, entry] of this.entries) {
      entry.container.classList.toggle("hidden", eid !== id);
    }
    requestAnimationFrame(() => {
      this.fitActive();
      // Don't steal the caret from a note editor, palette, or dialog field —
      // e.g. when `running` flips because the selected agent just exited.
      if (!isForeignTextEntryFocused(this.termsEl))
        this.entries.get(id)?.term.focus();
    });
  }

  /** Hide every terminal (e.g. when a worktree with no agent is selected). */
  hideAll() {
    this.activeId = null;
    for (const entry of this.entries.values()) {
      entry.container.classList.add("hidden");
    }
  }

  /** Dispose a terminal entirely (does not stop the agent). */
  closeTerminal(id: string) {
    // Drop the per-session opencode2 diagnostics regardless of whether a
    // terminal entry still exists: `checkOpencode2Hint` can run for a session
    // before its terminal is created. The id is `<project>/<feature>`, which a
    // future worktree can reuse, so stale `hintedSessions` would suppress the
    // hint forever and `hintBuffers` would just leak.
    this.hintBuffers.delete(id);
    this.hintedSessions.delete(id);
    const entry = this.entries.get(id);
    if (!entry) return;
    entry.term.dispose();
    entry.container.remove();
    this.entries.delete(id);
    if (this.activeId === id) this.activeId = null;
    this.onTerminalsChanged?.();
  }

  /** Scan a decoded output chunk for known opencode2 upstream errors and, on
   * first sight, surface a friendly "update opencode2" hint. Lightweight
   * diagnostic only — no attempt to parse or recover the provider stream.
   *
   * Gated on the current harness being `opencode2` so Claude/legacy-opencode
   * output never trips a false positive; the marker itself is then matched
   * against a rolling tail of decoded text so a marker split across two PTY
   * chunks still lands. */
  private checkOpencode2Hint(id: string, bytes: Uint8Array) {
    if (this.hintedSessions.has(id)) return;
    if (useStore.getState().settings.harness !== "opencode2") return;

    const text = new TextDecoder().decode(bytes);
    const tail = `${this.hintBuffers.get(id) ?? ""}${text}`.slice(
      -OPENCODE2_HINT_BUFFER,
    );
    this.hintBuffers.set(id, tail);

    const hint = detectOpencode2Hint(tail);
    if (hint) {
      this.hintedSessions.add(id);
      this.hintBuffers.delete(id);
      showError(hint, { sticky: true });
    }
  }

  async stop(id: string) {
    try {
      await invoke("kill_session", { id });
    } catch {
      /* already gone */
    }
  }

  /** Paste `text` into the currently active session's terminal, as if it
   * were typed/dropped there — same underlying mechanism as clipboard paste
   * and file drop (`entry.term.paste`) — then submit it by writing an Enter
   * keystroke to the PTY. Used by "add to chat" actions (a failed check's
   * log, PR comments) to hand context to the agent without the user
   * manually copy-pasting or pressing Enter themselves. Returns false
   * without writing anything when there's no active session. */
  pasteToActive(text: string): boolean {
    if (!this.activeId) return false;
    const entry = this.entries.get(this.activeId);
    if (!entry) return false;
    entry.term.paste(text);
    invoke("write_session", { id: this.activeId, data: "\r" }).catch(() => {});
    return true;
  }

  /** Submit a generated one-line prompt to a specific session: echo it into
   * that session's terminal (`term.paste`) so the injected prompt is visible
   * even though the agent TUI runs in raw mode, then confirm the Enter
   * keystroke reached the backend. Unlike `pasteToActive`, this never follows
   * mutable UI focus — it targets `id` directly — and reports a failed write
   * instead of silently swallowing it. */
  async submitTo(id: string, text: string): Promise<boolean> {
    const entry = this.entries.get(id);
    if (!entry) return false;
    try {
      entry.term.paste(text);
      await invoke("write_session", { id, data: "\r" });
      return true;
    } catch {
      return false;
    }
  }

  /** Remove the inline "Restart" button for `id`, if one is showing. */
  private clearRestartOverlay(id: string) {
    const entry = this.entries.get(id);
    if (!entry?.restartBtn) return;
    entry.restartBtn.remove();
    entry.restartBtn = undefined;
  }

  /** Show an inline "Restart" button over `id`'s terminal pane after a
   * non-deliberate exit (crash or the process just ending on its own) — a
   * one-click way to resume instead of switching to the toolbar. */
  private showRestartOverlay(id: string, entry: TermEntry) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn primary restart-overlay";
    btn.textContent = "Restart";
    btn.addEventListener("click", () => {
      this.clearRestartOverlay(id);
      this.onRestart?.(id);
    });
    entry.container.appendChild(btn);
    entry.restartBtn = btn;
  }

  /** Grow every terminal's font size by one step (e.g. Ctrl/Cmd +). */
  increaseFontSize() {
    this.setFontSize(this.fontSize + 1);
  }

  /** Shrink every terminal's font size by one step (e.g. Ctrl/Cmd -). */
  decreaseFontSize() {
    this.setFontSize(this.fontSize - 1);
  }

  /** Reset every terminal's font size to the default (e.g. Ctrl/Cmd 0). */
  resetFontSize() {
    this.setFontSize(DEFAULT_FONT_SIZE);
  }

  /** Force the active terminal to refit its container. Call after anything
   * outside `SessionManager` resizes the terminal's on-screen area — e.g. a
   * sibling panel expanding/collapsing — since xterm otherwise only refits
   * on `show()`, font changes, or the window `resize` event. */
  refit() {
    this.fitActive();
  }

  /** Force the active terminal to fully repaint, without touching its
   * buffer content or scrollback — a manual fix for the occasional WebGL
   * rendering glitch (stale/garbled glyphs) that a `resize`/refit doesn't
   * clear on its own. Clears the WebGL texture atlas (if that renderer is
   * active) and asks xterm to redraw every row. */
  repaintActive() {
    if (!this.activeId) return;
    const entry = this.entries.get(this.activeId);
    if (!entry) return;
    try {
      entry.webgl?.clearTextureAtlas();
    } catch {
      // Addon may already be disposed after a context-loss fallback.
    }
    entry.term.refresh(0, entry.term.rows - 1);
  }

  /** Set every terminal's font size directly (e.g. from the settings page). */
  setFontSize(size: number) {
    const clamped = Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, size));
    if (clamped === this.fontSize) return;
    this.fontSize = clamped;
    this.onFontSizeChange?.(clamped);
    for (const entry of this.entries.values()) {
      entry.term.options.fontSize = clamped;
    }
    this.fitActive();
  }

  /** Set every terminal's font family directly (e.g. from the settings
   * page). An empty string resets to the built-in monospace fallback. */
  setFontFamily(family: string) {
    const trimmed = family.trim();
    const applied = trimmed || FONT;
    if (applied === this.fontFamily) return;
    this.fontFamily = applied;
    this.onFontFamilyChange?.(trimmed);
    for (const entry of this.entries.values()) {
      entry.term.options.fontFamily = applied;
    }
    this.fitActive();
  }

  private async createTerminal(id: string) {
    const container = document.createElement("div");
    container.className =
      "term-pane xterm-fit absolute inset-0 py-1.5 px-2 hidden";
    this.termsEl.appendChild(container);

    const { term, fit } = createLinkedTerminal({
      fontFamily: this.fontFamily,
      fontSize: this.fontSize,
      cursorBlink: true,
      theme: THEME,
    });
    term.open(container);
    const webgl = loadRenderer(term);
    term.attachCustomKeyEventHandler((e) => this.handleKeyEvent(id, e));
    term.onData((data) => {
      this.onInput?.(id);
      invoke("write_session", { id, data }).catch(() => {});
    });

    this.entries.set(id, { term, fit, container, webgl });
    this.onTerminalsChanged?.();

    try {
      const attach = await invoke("attach_session", { id });
      if (attach.buffer.length) term.write(new Uint8Array(attach.buffer));
    } catch (err) {
      term.write(`\x1b[31mfailed to attach: ${err}\x1b[0m\r\n`);
    }
  }

  /** Re-send `entry`'s already-fitted current size to `resize_session`,
   * bypassing `fitActive`'s dedup (the size hasn't changed — only whether
   * the child actually received it has). See `TermEntry.resizeReannounced`. */
  private reannounceSize(id: string, entry: TermEntry) {
    const { rows, cols } = entry.term;
    entry.lastRows = rows;
    entry.lastCols = cols;
    invoke("resize_session", { id, rows, cols }).catch(() => {});
  }

  private fitActive() {
    if (!this.activeId) return;
    const entry = this.entries.get(this.activeId);
    if (!entry) return;
    // A hidden pane (a non-Agent tab, or the settings panel) measures 0×0 —
    // `fit()` would then drive the terminal down to xterm's 1×1 minimum and
    // resize the live PTY to match for a frame. Skip; `show()` and the
    // ResizeObserver both refit once it's visible again.
    if (this.termsEl.clientWidth === 0 || this.termsEl.clientHeight === 0) {
      return;
    }
    try {
      entry.fit.fit();
    } catch {
      return;
    }
    const { rows, cols } = entry.term;
    // Skip the round-trip (and the PTY SIGWINCH it triggers, which makes
    // claude redraw and momentarily looks like fresh agent activity) when
    // the size hasn't actually changed — e.g. just switching which already-
    // correctly-sized terminal is visible.
    if (entry.lastRows === rows && entry.lastCols === cols) return;
    entry.lastRows = rows;
    entry.lastCols = cols;
    invoke("resize_session", {
      id: this.activeId,
      rows,
      cols,
    }).catch(() => {});
  }
}

/**
 * GPU-accelerated rendering makes typing feel instant; xterm's default DOM
 * renderer redraws via the CPU and lags noticeably under webkit2gtk. Falls
 * back to the default renderer if WebGL is unavailable or the context drops.
 */
function loadRenderer(term: Terminal): WebglAddon | undefined {
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      console.warn(
        "[corral] webgl context lost, falling back to default renderer",
      );
      webgl.dispose();
    });
    term.loadAddon(webgl);
    console.info("[corral] webgl renderer active");
    return webgl;
  } catch (err) {
    console.warn(
      "[corral] webgl renderer unavailable, using default renderer",
      err,
    );
    return undefined;
  }
}

/** Encode raw RGBA pixels (as returned by the clipboard plugin) to PNG bytes via canvas. */
async function rgbaToPng(
  rgba: Uint8Array,
  width: number,
  height: number,
): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d")!;
  ctx.putImageData(
    new ImageData(new Uint8ClampedArray(rgba), width, height),
    0,
    0,
  );
  const blob = await new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("canvas.toBlob failed"))),
      "image/png",
    ),
  );
  return new Uint8Array(await blob.arrayBuffer());
}
