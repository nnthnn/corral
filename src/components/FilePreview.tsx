import clsx from "clsx";
import { useMemo, useState } from "react";
import { languageForPath } from "../lib/diffView";
import {
  imageDataUrl,
  isMarkdownPath,
  isSvgPath,
  svgDataUrl,
} from "../lib/files";
import { highlightCode } from "../lib/highlight";
import { currentWt, useStore } from "../store";
import styles from "./FilePreview.module.css";
import { MarkdownPreview } from "./MarkdownPreview";

/** Above this many lines, skip highlighting and render plain escaped text —
 * same guard (and rationale) as `DiffViewer`'s. A whole file is highlighted
 * in one pass rather than line-by-line, so multi-line constructs (block
 * comments, template strings) tokenize correctly. */
const MAX_HIGHLIGHT_LINES = 3000;

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Text formats that have both a rendered and a source view. Raster images
 * aren't here — they only render. */
function renderKindFor(path: string): "svg" | "markdown" | null {
  if (isSvgPath(path)) return "svg";
  if (isMarkdownPath(path)) return "markdown";
  return null;
}

/** Pre-highlighted file HTML, rendered as one `.diff-line` block so newlines
 * and multi-line tokens (block comments, template strings) survive. Split out
 * from the caller so the `dangerouslySetInnerHTML` element stays short enough
 * to keep `DiffViewer`'s biome-suppression pattern intact. */
function HighlightedText({ html }: { html: string }) {
  return (
    <span className="diff-line">
      {/* biome-ignore lint/security/noDangerouslySetInnerHtml: hljs output is fully HTML-escaped (see highlight.ts). */}
      <span dangerouslySetInnerHTML={{ __html: html }} />
    </span>
  );
}

/** Preview/Source segmented control for a file that has both views (SVG,
 * markdown). */
function ViewToggle({
  source,
  onChange,
}: {
  source: boolean;
  onChange: (source: boolean) => void;
}) {
  const tabClass = (selected: boolean) =>
    clsx(
      "inline-flex cursor-pointer items-center justify-center rounded-sm px-2 py-1 text-label font-medium transition-colors duration-150",
      selected ? "bg-surface-2 text-text" : "text-muted hover:text-text",
    );

  return (
    <nav
      className="file-view-toggle mr-2 flex items-center gap-0.5 rounded-md bg-bg p-0.5"
      aria-label="Preview or source"
    >
      <button
        type="button"
        className={tabClass(!source)}
        aria-pressed={!source}
        onClick={() => onChange(false)}
      >
        Preview
      </button>
      <button
        type="button"
        className={tabClass(source)}
        aria-pressed={source}
        onClick={() => onChange(true)}
      >
        Source
      </button>
    </nav>
  );
}

export function FilePreview() {
  const wt = useStore(currentWt);
  const fileTab = useStore((s) => s.fileTab);
  const fileContent = useStore((s) => s.fileContent);
  const fileError = useStore((s) => s.fileError);
  const fileFontSize = useStore((s) => s.settings.diff_font_size);
  // Records *which* path was flipped to Source rather than a bare boolean:
  // opening a different file then starts back in Preview with no reset effect
  // to get wrong — the stale path simply stops matching.
  const [sourceFor, setSourceFor] = useState<string | null>(null);

  const path = fileTab?.path ?? "";
  const text = fileContent?.kind === "text" ? fileContent.text : null;
  const renderKind = text === null ? null : renderKindFor(path);
  const showSource = renderKind !== null && sourceFor === path;

  // Raster images always render; SVG/markdown render unless flipped to Source.
  const showRasterImage = fileContent?.kind === "image";
  const showSvg = renderKind === "svg" && !showSource;
  const showMarkdown = renderKind === "markdown" && !showSource;
  const showImage = showRasterImage || showSvg;

  // Pre-highlighted HTML for the source body, or `null` to render plain lines
  // (unknown language, or too big to tokenize). `.svg` maps to `xml` and
  // `.md` to `markdown` via `languageForPath`, so both Source views highlight.
  const highlighted = useMemo(() => {
    if (text === null) return null;
    const language = languageForPath(path);
    if (!language) return null;
    if (text.split("\n").length > MAX_HIGHLIGHT_LINES) return null;
    return highlightCode(text, language);
  }, [text, path]);

  return (
    <>
      <div className="diff-head flex items-center gap-1 border-b border-border bg-surface">
        <p className="diff-path flex-1 m-0 py-3 pl-4 pr-2 font-mono text-sub whitespace-nowrap overflow-hidden text-ellipsis">
          {path}
        </p>
        {renderKind !== null && (
          <ViewToggle
            source={showSource}
            onChange={(source) => setSourceFor(source ? path : null)}
          />
        )}
      </div>
      {showImage ? (
        <div className={styles.imageBody}>
          <img
            className={styles.image}
            src={
              fileContent?.kind === "image"
                ? imageDataUrl(fileContent.mime, fileContent.data)
                : fileContent?.kind === "text"
                  ? svgDataUrl(fileContent.text)
                  : ""
            }
            alt={path}
          />
        </div>
      ) : showMarkdown ? (
        <MarkdownPreview
          wt={wt}
          path={path}
          text={text ?? ""}
          className={styles.markdownPreview}
        />
      ) : (
        <pre
          className="diff-body flex-1 m-0 overflow-auto py-2 font-mono leading-[1.5]"
          style={{ fontSize: fileFontSize }}
        >
          {fileError ? (
            <span className="diff-line err">{fileError}</span>
          ) : fileContent === null ? (
            <span className="diff-loading">Loading…</span>
          ) : fileContent.kind === "binary" ? (
            <span className="diff-loading">Binary file, not previewed.</span>
          ) : fileContent.kind === "too_large" ? (
            <span className="diff-loading">
              File too large to preview ({formatBytes(fileContent.size)}).
            </span>
          ) : highlighted !== null ? (
            <HighlightedText html={highlighted} />
          ) : (
            fileContent.text.split("\n").map((line, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: whole list is replaced wholesale on every new file load, never reordered.
              <span key={i} className="diff-line">
                {line || " "}
              </span>
            ))
          )}
        </pre>
      )}
    </>
  );
}
