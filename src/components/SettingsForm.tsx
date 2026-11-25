import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import clsx from "clsx";
import { ArrowLeft, Check, Copy, Loader2, Volume2 } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import {
  FALLBACK_MONOSPACE_FONTS,
  firstUninstalledFamily,
  type MonospaceFonts,
  monospaceFonts,
} from "../lib/fonts";
import { playSoundChoice, SOUND_OPTIONS } from "../lib/notifications";
import { cancelScheduledSettingsSave, saveSettings } from "../lib/settings";
import { invoke } from "../lib/tauri";
import { useStore } from "../store";
import type { PlatformDefaults, Settings, SoundChoice } from "../types";
import { Button } from "./Button";
import styles from "./SettingsForm.module.css";

type SaveState = "idle" | "saving" | "saved" | "error";

export function SettingsForm() {
  const initial = useStore.getState().settings;
  const [editorCommand, setEditorCommand] = useState(initial.editor_command);
  const [notificationsEnabled, setNotificationsEnabled] = useState(
    initial.notifications_enabled,
  );
  const [fontSize, setFontSize] = useState(String(initial.terminal_font_size));
  const [fontFamily, setFontFamily] = useState(initial.terminal_font_family);
  // Installed monospaced families, fetched once per mount (the backend caches
  // the actual scan). Starts as the fallback list so the picker is usable
  // immediately and in tests.
  const [terminalFonts, setTerminalFonts] = useState<MonospaceFonts>({
    families: FALLBACK_MONOSPACE_FONTS,
    enumerated: false,
  });
  // The backend's platform defaults for the two command fields. A blank field
  // must persist *these* (macOS: `zed`/`open`), not a hardcoded Linux value —
  // the backend's `Settings::default()` seeds the same pair, so saving the
  // default verbatim round-trips instead of silently pinning `ghostty` on a
  // Mac. Starts `null` and falls back to the loaded settings' own values, so a
  // slow/mocked backend can't make the field look empty.
  const [platformDefaults, setPlatformDefaults] =
    useState<PlatformDefaults | null>(null);
  const [diffFontSize, setDiffFontSize] = useState(
    String(initial.diff_font_size),
  );
  const [worktreesRoot, setWorktreesRoot] = useState(initial.worktrees_root);
  const [terminalCommand, setTerminalCommand] = useState(
    initial.terminal_command,
  );
  const [claudeExecutablePath, setClaudeExecutablePath] = useState(
    initial.claude_executable_path,
  );
  const [harness, setHarness] = useState<Settings["harness"]>(initial.harness);
  const [opencodeExecutablePath, setOpencodeExecutablePath] = useState(
    initial.opencode_executable_path,
  );
  const [opencode2ExecutablePath, setOpencode2ExecutablePath] = useState(
    initial.opencode2_executable_path,
  );
  const [piExecutablePath, setPiExecutablePath] = useState(
    initial.pi_executable_path,
  );
  const [linearApiKey, setLinearApiKey] = useState(
    initial.linear_api_key ?? "",
  );
  const [extraInstructions, setExtraInstructions] = useState(
    initial.extra_instructions,
  );
  const [skipPermissions, setSkipPermissions] = useState(
    initial.skip_permissions,
  );
  // `!== false`: an older backend that doesn't send this field should read as
  // enabled (the pre-setting behavior), matching `auto_setup`'s handling.
  const [browserEnabled, setBrowserEnabled] = useState(
    initial.browser_enabled !== false,
  );
  const [coAuthorTrailer, setCoAuthorTrailer] = useState(
    initial.co_author_trailer,
  );
  const [soundEnabled, setSoundEnabled] = useState(initial.sound_enabled);
  const [soundChoice, setSoundChoice] = useState<SoundChoice>(
    initial.sound_choice,
  );
  const [scrollbackKb, setScrollbackKb] = useState(
    String(initial.scrollback_kb),
  );
  const [deleteBranchDefault, setDeleteBranchDefault] = useState(
    initial.delete_branch_default,
  );
  const [caffeinateEnabled, setCaffeinateEnabled] = useState(
    initial.caffeinate_enabled,
  );
  const [idleTimeoutEnabled, setIdleTimeoutEnabled] = useState(
    initial.idle_timeout_enabled,
  );
  const [idleTimeoutMinutes, setIdleTimeoutMinutes] = useState(
    String(initial.idle_timeout_minutes),
  );
  const [error, setError] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [diagState, setDiagState] = useState<"idle" | "copying" | "copied">(
    "idle",
  );

  /** Gather the diagnostics blob (same snapshot as `corral doctor`, plus the
   * app's log tail) and put it on the clipboard, ready to paste into an
   * issue. Deliberately not part of the settings form: it's an action, not a
   * preference, and it doesn't need saving. */
  async function copyDiagnostics() {
    setDiagState("copying");
    try {
      const text = await invoke("copy_diagnostics");
      await writeText(text);
      setDiagState("copied");
    } catch (err) {
      setError(String(err));
      setDiagState("idle");
    }
    setTimeout(() => setDiagState("idle"), 2000);
  }

  useEffect(() => {
    let cancelled = false;
    monospaceFonts().then((fonts) => {
      if (!cancelled) setTerminalFonts(fonts);
    });
    // Advisory only: if the backend can't answer, the save path falls back to
    // whatever is already in the field, so nothing is lost.
    invoke("platform_defaults")
      .then((defaults) => {
        if (!cancelled) setPlatformDefaults(defaults);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // Only warn when the list really came from this machine's scan — with the
  // fallback list we have no idea what's installed, so claiming anything is
  // missing would be wrong.
  const missingFont = terminalFonts.enumerated
    ? firstUninstalledFamily(fontFamily, terminalFonts.families)
    : null;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setSaveState("saving");
    const trimmedWorktreesRoot = worktreesRoot.trim();
    if (
      trimmedWorktreesRoot !== "" &&
      !(
        trimmedWorktreesRoot.startsWith("/") ||
        trimmedWorktreesRoot.startsWith("~")
      )
    ) {
      setError("Worktrees root must be an absolute path or start with ~");
      setSaveState("error");
      setTimeout(() => setSaveState("idle"), 1500);
      return;
    }
    const s = useStore.getState();
    const parsedFontSize = Number(fontSize);
    const parsedScrollbackKb = Number(scrollbackKb);
    const parsedDiffFontSize = Number(diffFontSize);
    const parsedIdleTimeoutMinutes = Number(idleTimeoutMinutes);

    // Routed through SessionManager (not written into `settings` directly)
    // so it clamps/applies consistently with Ctrl/Cmd +/-/0 (font size) and
    // takes effect on already-open terminals immediately.
    if (Number.isFinite(parsedFontSize) && parsedFontSize > 0) {
      s.sessionMgr?.setFontSize(parsedFontSize);
    }
    s.sessionMgr?.setFontFamily(fontFamily);

    // Re-read after the sessionMgr calls above: setFontSize/setFontFamily
    // synchronously update the store (via onFontSizeChange/onFontFamilyChange
    // in TerminalHost.tsx) when they detect a change, so `s.settings` captured
    // before those calls is stale. Spreading from a fresh read here ensures
    // we don't clobber the font values they just applied.
    // Fall back to the loaded value (not a hardcoded string) so a failed
    // `platform_defaults` fetch can never blank out a real setting. If the
    // stored value is *also* blank, save the platform default so a
    // hand-edited `settings.json` with "" gets repaired on the next Save
    // instead of persisting an empty command.
    const current = useStore.getState().settings;
    const editorFallback =
      platformDefaults?.editor_command || current.editor_command;
    const terminalFallback =
      platformDefaults?.terminal_command || current.terminal_command;
    const updated = {
      ...current,
      editor_command: editorCommand.trim() || editorFallback || "zeditor",
      notifications_enabled: notificationsEnabled,
      extra_instructions: extraInstructions.trim(),
      skip_permissions: skipPermissions,
      browser_enabled: browserEnabled,
      co_author_trailer: coAuthorTrailer,
      sound_enabled: soundEnabled,
      sound_choice: soundChoice,
      scrollback_kb:
        Number.isFinite(parsedScrollbackKb) && parsedScrollbackKb >= 64
          ? Math.round(parsedScrollbackKb)
          : current.scrollback_kb,
      diff_font_size:
        Number.isFinite(parsedDiffFontSize) &&
        parsedDiffFontSize >= 8 &&
        parsedDiffFontSize <= 32
          ? Math.round(parsedDiffFontSize)
          : current.diff_font_size,
      delete_branch_default: deleteBranchDefault,
      caffeinate_enabled: caffeinateEnabled,
      worktrees_root: trimmedWorktreesRoot,
      terminal_command: terminalCommand.trim() || terminalFallback || "ghostty",
      idle_timeout_enabled: idleTimeoutEnabled,
      idle_timeout_minutes:
        Number.isFinite(parsedIdleTimeoutMinutes) &&
        parsedIdleTimeoutMinutes >= 1
          ? Math.round(parsedIdleTimeoutMinutes)
          : current.idle_timeout_minutes,
      claude_executable_path: claudeExecutablePath.trim(),
      harness,
      opencode_executable_path: opencodeExecutablePath.trim(),
      opencode2_executable_path: opencode2ExecutablePath.trim(),
      pi_executable_path: piExecutablePath.trim(),
      linear_api_key: linearApiKey.trim(),
    };
    try {
      useStore.setState({ settings: updated });
      await saveSettings();
      cancelScheduledSettingsSave();
      setError(null);
      setSaveState("saved");
      setTimeout(() => setSaveState("idle"), 1500);
    } catch (err) {
      setError(String(err));
      setSaveState("error");
      setTimeout(() => setSaveState("idle"), 1500);
    }
  }

  return (
    <form onSubmit={handleSubmit} className={styles.form}>
      <div className="settings-header">
        <div className="settings-title-row flex items-center gap-3">
          <Button
            variant="ghost"
            square
            title="Back (Esc)"
            onClick={() => useStore.setState({ settingsOpen: false })}
          >
            <ArrowLeft size={16} />
          </Button>
          <h2>Settings</h2>
          <kbd className="settings-esc-hint" title="Press Esc to close">
            Esc
          </kbd>
          <Button
            type="submit"
            variant="primary"
            className={clsx(
              styles.saveBtn,
              saveState === "error" && styles.saveBtnError,
              saveState === "saved" && styles.saveBtnSuccess,
            )}
            disabled={saveState === "saving"}
          >
            {saveState === "saving" && (
              <Loader2 size={14} className="animate-spin" />
            )}
            {saveState === "saved" && (
              <Check size={14} className={styles.checkIcon} />
            )}
            {saveState === "saving"
              ? "Saving…"
              : saveState === "saved"
                ? "Saved"
                : "Save"}
          </Button>
        </div>
      </div>
      <div className={styles.body}>
        <div className={styles.columns}>
          <div className={styles.column}>
            <h3 className={styles.columnHeading}>Editor &amp; Terminal</h3>
            <label>
              Editor command
              <input
                list="editor-command-list"
                placeholder={platformDefaults?.editor_command ?? "zeditor"}
                required
                value={editorCommand}
                onChange={(e) => setEditorCommand(e.target.value)}
              />
              <datalist id="editor-command-list">
                <option value="zed" />
                <option value="zeditor" />
              </datalist>
            </label>
            <p className="hint">
              Run by the "Zed" button (<code>zed</code> vs <code>zeditor</code>,
              depending on your install).
            </p>
            <label>
              Terminal command
              <input
                list="terminal-command-list"
                placeholder={platformDefaults?.terminal_command ?? "ghostty"}
                required
                value={terminalCommand}
                onChange={(e) => setTerminalCommand(e.target.value)}
              />
              <datalist id="terminal-command-list">
                <option value="ghostty" />
                <option value="alacritty" />
                <option value="kitty" />
                <option value="open" />
              </datalist>
            </label>
            <p className="hint">
              Run by the "Shell" button using <code>--working-directory</code>{" "}
              (Ghostty/Alacritty/Kitty); other terminals may need a different
              flag. On macOS, <code>open</code> opens Terminal at the worktree
              path.
            </p>
            <label>
              Worktrees root
              <input
                placeholder="~/corral"
                value={worktreesRoot}
                onChange={(e) => setWorktreesRoot(e.target.value)}
              />
            </label>
            <p className="hint">
              Where new worktrees are created (blank = ~/corral). Existing
              worktrees elsewhere won't move — point this at them to keep using
              them.
            </p>
            <label>
              Terminal font size
              <input
                type="number"
                min={8}
                max={32}
                step={1}
                required
                value={fontSize}
                onChange={(e) => setFontSize(e.target.value)}
              />
            </label>
            <p className="hint">
              Also adjustable per-session with <code>Ctrl/Cmd +</code>,{" "}
              <code>Ctrl/Cmd -</code>, and <code>Ctrl/Cmd 0</code>.
            </p>
            <label>
              Diff font size
              <input
                type="number"
                min={8}
                max={32}
                step={1}
                required
                value={diffFontSize}
                onChange={(e) => setDiffFontSize(e.target.value)}
              />
            </label>
            <p className="hint">
              Changes pane diff text size. <code>Ctrl/Cmd +</code>,{" "}
              <code>Ctrl/Cmd -</code>, and <code>Ctrl/Cmd 0</code> while a diff
              is open.
            </p>
          </div>
          <div className={styles.column}>
            <h3 className={styles.columnHeading}>Agent</h3>
            <label>
              Harness
              <select
                value={harness}
                onChange={(e) =>
                  setHarness(e.target.value as Settings["harness"])
                }
              >
                <option value="claude">Claude Code</option>
                <option value="opencode2">OpenCode v2</option>
                {/* OpenCode v1 is retired — no longer offered for new
                    selections. It stays in the list only while it's the
                    current value, so an existing selection renders instead of
                    the <select> falling back to blank; the backend still
                    spawns it (Harness::Opencode) for those settings. */}
                {harness === "opencode" && (
                  <option value="opencode">OpenCode v1 (legacy)</option>
                )}
                <option value="pi">Pi</option>
              </select>
            </label>
            <p className="hint">
              Which harness new sessions launch with. Applies to new sessions
              only.
            </p>
            {harness === "claude" && (
              <>
                <label>
                  Claude Code executable
                  <input
                    placeholder="claude"
                    value={claudeExecutablePath}
                    onChange={(e) => setClaudeExecutablePath(e.target.value)}
                  />
                </label>
                <p className="hint">
                  Path or command; blank uses <code>claude</code> from{" "}
                  <code>PATH</code>. Point at a custom build if needed. New
                  sessions only.
                </p>
              </>
            )}
            {harness === "opencode2" && (
              <>
                <label>
                  OpenCode v2 executable
                  <input
                    placeholder="opencode"
                    value={opencode2ExecutablePath}
                    onChange={(e) => setOpencode2ExecutablePath(e.target.value)}
                  />
                </label>
                <p className="hint">
                  Path or command; blank uses <code>opencode</code> from{" "}
                  <code>PATH</code>. Install via{" "}
                  <code>pnpm add -g @opencode/cli</code>. New sessions only.
                </p>
                <label className="checkbox-row">
                  <input
                    type="checkbox"
                    checked={browserEnabled}
                    onChange={(e) => setBrowserEnabled(e.target.checked)}
                  />
                  <span className="min-w-0">
                    Attach corral's embedded browser (OpenCode browser tools)
                  </span>
                </label>
                <p className="hint">
                  Lets the agent drive a corral-owned browser window via
                  OpenCode's <code>browser.*</code> tools. Turning this off
                  leaves those tools unavailable and starts nothing extra. New
                  sessions only.
                </p>
              </>
            )}
            {harness === "opencode" && (
              <>
                <label>
                  OpenCode v1 executable
                  <input
                    placeholder="opencode"
                    value={opencodeExecutablePath}
                    onChange={(e) => setOpencodeExecutablePath(e.target.value)}
                  />
                </label>
                <p className="hint">
                  Path or command; blank uses <code>opencode</code> from{" "}
                  <code>PATH</code>. OpenCode v1 (legacy) is no longer shipped —
                  current <code>opencode</code> is v2, so point this at a v1
                  binary. New sessions only.
                </p>
              </>
            )}
            {harness === "pi" && (
              <>
                <label>
                  Pi executable
                  <input
                    placeholder="pi"
                    value={piExecutablePath}
                    onChange={(e) => setPiExecutablePath(e.target.value)}
                  />
                </label>
                <p className="hint">
                  Path or command; blank uses <code>pi</code> from{" "}
                  <code>PATH</code>. Install with{" "}
                  <code>curl -fsSL https://pi.dev/install.sh | sh</code>. New
                  sessions only.
                </p>
              </>
            )}
            <label>
              Scrollback buffer (KB)
              <input
                type="number"
                min={64}
                max={2048}
                step={64}
                required
                value={scrollbackKb}
                onChange={(e) => setScrollbackKb(e.target.value)}
              />
            </label>
            <p className="hint">
              Recent terminal output kept per session for re-attach. Applies to
              sessions started after saving.
            </p>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={coAuthorTrailer}
                onChange={(e) => setCoAuthorTrailer(e.target.checked)}
              />
              Add "Co-Authored-By: Corral" to commits Claude creates
            </label>
            <p className="hint">
              Instructs fresh sessions to include the trailer. Doesn't apply
              retroactively.
            </p>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={skipPermissions}
                disabled={harness !== "claude" && harness !== "opencode2"}
                onChange={(e) => setSkipPermissions(e.target.checked)}
              />
              Skip permission prompts
            </label>
            <p className="warn">
              Bypasses permission prompts (Claude Code:{" "}
              <code>--dangerously-skip-permissions</code>, OpenCode v2:{" "}
              <code>--auto</code>). Trusted worktrees only; new sessions only.{" "}
              {harness === "opencode" && (
                <>
                  OpenCode v1 (legacy) has no equivalent flag, so this has no
                  effect while that harness is selected above.
                </>
              )}
              {harness === "pi" && (
                <>
                  Pi has no built-in permission prompts, so this has no effect
                  while that harness is selected above.
                </>
              )}
            </p>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={deleteBranchDefault}
                onChange={(e) => setDeleteBranchDefault(e.target.checked)}
              />
              <span className="min-w-0">
                Default "Also delete branch" to checked when removing a worktree
              </span>
            </label>
            <label>
              Linear API key
              <input
                type="password"
                placeholder="lin_api_…"
                autoComplete="off"
                value={linearApiKey}
                onChange={(e) => setLinearApiKey(e.target.value)}
              />
            </label>
            <p className="hint">
              Enables the header's ticket button and the command palette's
              "Attach Linear ticket". Create a personal key under Linear →
              Settings → Security &amp; access → Personal API keys. Stored in
              plain text in this machine's <code>settings.json</code>.
            </p>
          </div>
          <div className={styles.column}>
            <h3 className={styles.columnHeading}>Notifications &amp; Power</h3>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={notificationsEnabled}
                onChange={(e) => setNotificationsEnabled(e.target.checked)}
              />
              Notify me when an agent starts waiting
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={soundEnabled}
                onChange={(e) => setSoundEnabled(e.target.checked)}
              />
              Play a sound when an agent starts waiting
            </label>
            <label>
              Sound
              <span className={styles.soundRow}>
                <select
                  value={soundChoice}
                  onChange={(e) =>
                    setSoundChoice(e.target.value as SoundChoice)
                  }
                >
                  {SOUND_OPTIONS.map((opt) => (
                    <option key={opt.id} value={opt.id}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                <Button
                  type="button"
                  variant="ghost"
                  icon
                  title="Preview this sound"
                  onClick={() => playSoundChoice(soundChoice)}
                >
                  <Volume2 size={14} />
                </Button>
              </span>
            </label>
            <p className="hint">
              Independent of the notification popup — combine both, either, or
              neither.
            </p>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={caffeinateEnabled}
                onChange={(e) => setCaffeinateEnabled(e.target.checked)}
              />
              Prevent sleep while any agent is working
            </label>
            <p className="hint">
              <code>systemd-inhibit</code> to block suspend/lock while at least
              one agent is actively working (not while it's just parked idle at
              its prompt). No effect without systemd.
            </p>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={idleTimeoutEnabled}
                onChange={(e) => setIdleTimeoutEnabled(e.target.checked)}
              />
              <span className="min-w-0">Auto-stop idle sessions</span>
            </label>
            {idleTimeoutEnabled && (
              <label>
                Stop after (minutes)
                <input
                  type="number"
                  min={1}
                  max={1440}
                  step={1}
                  required
                  value={idleTimeoutMinutes}
                  onChange={(e) => setIdleTimeoutMinutes(e.target.value)}
                />
              </label>
            )}
            <p className="hint">
              Stops an agent once it's sat waiting untouched for this long — a
              guard against burning budget.
            </p>
            <label>
              Terminal font family
              <input
                list="terminal-font-family-list"
                placeholder="(default monospace)"
                value={fontFamily}
                onChange={(e) => setFontFamily(e.target.value)}
              />
              <datalist id="terminal-font-family-list">
                {terminalFonts.families.map((family) => (
                  <option key={family} value={family} />
                ))}
              </datalist>
            </label>
            <p className="hint">
              Blank = built-in monospace. Suggestions are the monospaced fonts
              installed on this machine — you can also type any family name.
              Unavailable fonts fall back to monospace silently.
            </p>
            {missingFont && (
              <p className="warn">
                “{missingFont}” isn't among this machine's installed monospaced
                fonts — the terminal will fall back to its built-in monospace.
                You can still keep it here if the font is installed elsewhere.
              </p>
            )}
          </div>
        </div>
        <label>
          Extra instructions for Claude
          <textarea
            rows={6}
            placeholder="e.g. always run cargo check before declaring a Rust change done"
            value={extraInstructions}
            onChange={(e) => setExtraInstructions(e.target.value)}
          />
        </label>
        <p className="hint">
          Appended to every new session's system prompt, alongside Corral's own
          briefing. Doesn't apply retroactively.
        </p>
        <div className={styles.diagnostics}>
          <Button
            type="button"
            variant="ghost"
            onClick={copyDiagnostics}
            disabled={diagState === "copying"}
          >
            {diagState === "copied" ? <Check size={14} /> : <Copy size={14} />}
            {diagState === "copying"
              ? "Collecting…"
              : diagState === "copied"
                ? "Copied"
                : "Copy diagnostics"}
          </Button>
          <p className="hint">
            Copies versions, resolved executables, desktop/WebKit info, config
            health, and the tail of this app's log — the same thing{" "}
            <code>corral doctor</code> prints. Contains no project names, file
            contents, or prompts. Paste it into a bug report.
          </p>
        </div>
        {error && <p className="error">{error}</p>}
      </div>
    </form>
  );
}
