/** Known upstream OpenCode v2 errors worth nudging the user to update over.
 *
 * Kept deliberately minimal: Corral only *detects* these in the embedded TUI
 * output and suggests updating OpenCode v2 — it never tries to parse or
 * recover the provider stream itself (that's OpenCode v2's job, and pinning a
 * fix here would just go stale against upstream).
 */

interface KnownError {
  /** The literal error substring to look for in decoded PTY output. */
  marker: string;
  /** Friendly hint to surface once, per session, when `marker` is seen. */
  hint: string;
}

const KNOWN_ERRORS: KnownError[] = [
  {
    marker: "Invalid opencode/openai-compatible-chat stream event",
    hint: "OpenCode v2 hit a known upstream error (invalid stream event) — updating OpenCode v2 usually fixes it.",
  },
];

/** Strip ANSI escape sequences (colors, cursor movement) so a TUI-rendered
 * error line still matches its plain-text marker. Built from a string (rather
 * than a regex literal) so the ESC byte doesn't trip the
 * `noControlCharactersInRegex` lint. */
const ESC = "\u001b";
const ANSI_RE = new RegExp(`${ESC}\\[[0-9;?]*[ -/]*[@-~]`, "g");

/** Return a friendly "update opencode2" hint if `text` contains a known
 * upstream error, else `null`. Pure, so it's unit-testable without a PTY. */
export function detectOpencode2Hint(text: string): string | null {
  const stripped = text.replace(ANSI_RE, "");
  for (const { marker, hint } of KNOWN_ERRORS) {
    if (stripped.includes(marker)) return hint;
  }
  return null;
}
