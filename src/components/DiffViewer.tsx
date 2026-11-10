import clsx from "clsx";
import { ChevronDown, ChevronUp, EyeOff } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { stepDiff } from "../lib/diff";
import { type DiffLineKind, languageForPath, splitDiff } from "../lib/diffView";
import { highlightCode } from "../lib/highlight";
import { useStore } from "../store";
import { Button } from "./Button";

/** Above this many raw diff lines, per-line syntax highlighting is skipped
 * (lines still get their add/del/ctx coloring) so opening a huge diff doesn't
 * spend hundreds of ms tokenizing. */
const MAX_HIGHLIGHT_LINES = 3000;

/** Whether a `keydown`'s target is an input-like element that should keep the
 * keystroke to itself (e.g. the sidebar filter) rather than drive diff nav. */
function isEditableTarget(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  if (
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    t instanceof HTMLSelectElement
  ) {
    return true;
  }
  return t.isContentEditable;
}

type Row =
  | { kind: "plain"; cls: DiffLineKind; text: string }
  | { kind: "code"; cls: DiffLineKind; prefix: string; html: string };

export function DiffViewer() {
  const diffTab = useStore((s) => s.diffTab);
  const diffContent = useStore((s) => s.diffContent);
  const diffError = useStore((s) => s.diffError);
  const diffFontSize = useStore((s) => s.settings.diff_font_size);
  const [hideUnchanged, setHideUnchanged] = useState(false);

  const language = useMemo(
    () => languageForPath(diffTab?.path ?? ""),
    [diffTab?.path],
  );

  const rows = useMemo<Row[] | null>(() => {
    if (diffContent === null) return null;
    const lines = splitDiff(diffContent);
    const visible = hideUnchanged
      ? lines.filter((l) => l.kind !== "ctx")
      : lines;
    const highlight = lines.length <= MAX_HIGHLIGHT_LINES;
    return visible.map((l) => {
      if (!highlight || l.kind === "meta" || l.kind === "hunk") {
        return { kind: "plain", cls: l.kind, text: l.text || " " };
      }
      return {
        kind: "code",
        cls: l.kind,
        prefix: l.text[0] ?? " ",
        html: highlightCode(l.text.slice(1), language),
      };
    });
  }, [diffContent, hideUnchanged, language]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (useStore.getState().activeTab !== "diff") return;
      if (isEditableTarget(e)) return;
      let delta: -1 | 1 | 0 = 0;
      if (e.key === "j" || e.key === "ArrowDown") delta = 1;
      else if (e.key === "k" || e.key === "ArrowUp") delta = -1;
      else return;
      e.preventDefault();
      stepDiff(delta);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <>
      <div className="diff-head flex items-center gap-1 border-b border-border bg-surface">
        <p className="diff-path flex-1 m-0 py-3 pl-4 pr-2 font-mono text-sub whitespace-nowrap overflow-hidden text-ellipsis">
          {diffTab?.path ?? ""}
        </p>
        <div className="diff-tools flex items-center gap-1 pr-2">
          <Button
            variant="ghost"
            square
            title="Previous file (k / ↑)"
            onClick={() => stepDiff(-1)}
          >
            <ChevronUp size={14} />
          </Button>
          <Button
            variant="ghost"
            square
            title="Next file (j / ↓)"
            onClick={() => stepDiff(1)}
          >
            <ChevronDown size={14} />
          </Button>
          <Button
            variant="ghost"
            square
            active={hideUnchanged}
            aria-pressed={hideUnchanged}
            title={
              hideUnchanged ? "Show unchanged lines" : "Hide unchanged lines"
            }
            onClick={() => setHideUnchanged((v) => !v)}
          >
            <EyeOff size={14} />
          </Button>
        </div>
      </div>
      <pre
        className="diff-body flex-1 m-0 overflow-auto py-2 font-mono leading-[1.5]"
        style={{ fontSize: diffFontSize }}
      >
        {diffError ? (
          <span className="diff-line err">{diffError}</span>
        ) : diffContent === null ? (
          <span className="diff-loading">Loading…</span>
        ) : !diffContent.trim() ? (
          <span className="diff-loading">
            No textual diff (binary or unchanged).
          </span>
        ) : rows && rows.length === 0 ? (
          <span className="diff-loading">No changed lines to show.</span>
        ) : (
          rows?.map((row, i) =>
            row.kind === "plain" ? (
              // biome-ignore lint/suspicious/noArrayIndexKey: whole list is replaced wholesale on every new diff load, never reordered.
              <span key={i} className={clsx("diff-line", row.cls)}>
                {row.text}
              </span>
            ) : (
              // biome-ignore lint/suspicious/noArrayIndexKey: whole list is replaced wholesale on every new diff load, never reordered.
              <span key={i} className={clsx("diff-line", row.cls)}>
                {row.prefix}
                {/* biome-ignore lint/security/noDangerouslySetInnerHtml: hljs output is fully HTML-escaped (see highlight.ts). */}
                <span dangerouslySetInnerHTML={{ __html: row.html }} />
              </span>
            ),
          )
        )}
      </pre>
    </>
  );
}
