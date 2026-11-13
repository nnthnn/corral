import clsx from "clsx";
import { ArrowLeft, Check, Loader2, Volume2 } from "lucide-react";
import { type FormEvent, useState } from "react";
import { playSoundChoice, SOUND_OPTIONS } from "../lib/notifications";
import { cancelScheduledSettingsSave, saveSettings } from "../lib/settings";
import { useStore } from "../store";
import type { Settings, SoundChoice } from "../types";
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
  const [extraInstructions, setExtraInstructions] = useState(
    initial.extra_instructions,
  );
  const [skipPermissions, setSkipPermissions] = useState(
    initial.skip_permissions,
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
    const current = useStore.getState().settings;
    const updated = {
      ...current,
      editor_command: editorCommand.trim() || "zeditor",
      notifications_enabled: notificationsEnabled,
      extra_instructions: extraInstructions.trim(),
      skip_permissions: skipPermissions,
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
      terminal_command: terminalCommand.trim() || "ghostty",
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
                placeholder="zeditor"
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
                placeholder="ghostty"
                required
                value={terminalCommand}
                onChange={(e) => setTerminalCommand(e.target.value)}
              />
              <datalist id="terminal-command-list">
                <option value="ghostty" />
                <option value="alacritty" />
                <option value="kitty" />
              </datalist>
            </label>
            <p className="hint">
              Run by the "Shell" button using <code>--working-directory</code>{" "}
              (Ghostty/Alacritty/Kitty); other terminals may need a different
              flag.
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
                <option value="JetBrains Mono" />
                <option value="Fira Code" />
                <option value="monospace" />
              </datalist>
            </label>
            <p className="hint">
              Blank = built-in monospace. Unavailable fonts fall back silently.
            </p>
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
        {error && <p className="error">{error}</p>}
      </div>
    </form>
  );
}
