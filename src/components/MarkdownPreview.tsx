import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import "github-markdown-css/github-markdown-dark.css";
import { renderMarkdown, resolveMarkdownImages } from "../lib/markdown";
import type { Worktree } from "../types";
import styles from "./MarkdownPreview.module.css";

const EXTERNAL_HREF = /^(https?:|mailto:)/i;

/** Rendered GFM view for markdown from a file or a PR/comment body. The
 * sanitized HTML is written to the node imperatively (rather than via
 * `dangerouslySetInnerHTML`) so the body itself carries the prose styles and
 * there's no wrapper element between the styles and their targets.
 *
 * Layout is the caller's business — the root is a plain block, so a file pane
 * can make it a scrolling region and a PR pane can let it flow with its
 * surrounding content. `compact` drops to GitHub's issue/comment text size
 * for the denser panes. */
export function MarkdownPreview({
  wt,
  path,
  text,
  className,
  compact,
}: {
  wt: Worktree | undefined;
  path: string;
  text: string;
  className?: string;
  compact?: boolean;
}) {
  const baseHtml = useMemo(() => renderMarkdown(text), [text]);
  const [html, setHtml] = useState(baseHtml);
  const ref = useRef<HTMLDivElement>(null);
  // The store replaces the worktree objects on each poll, so depend on the
  // stable project/feature strings rather than the object identity — otherwise
  // every background refresh would re-resolve (and re-read) the images.
  const project = wt?.project;
  const feature = wt?.feature;

  // Show the sanitized HTML immediately, then swap in worktree-resolved
  // `data:` URLs for any relative images once they've been read.
  useEffect(() => {
    setHtml(baseHtml);
    if (project === undefined || feature === undefined) return;
    let cancelled = false;
    void resolveMarkdownImages({ project, feature }, path, baseHtml).then(
      (resolved) => {
        if (!cancelled) setHtml(resolved);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [baseHtml, path, project, feature]);

  useEffect(() => {
    if (ref.current) ref.current.innerHTML = html;
  }, [html]);

  // Delegated on the container so it survives re-renders of the injected HTML.
  // Relative links are inert (they have no route here); external links open in
  // the system browser rather than navigating the webview. Anchors already
  // fire a click on Enter, so no separate key handler is needed — the `a11y`
  // rule doesn't see that because this is a plain DOM listener, not JSX.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    function onClick(e: Event) {
      const anchor = (e.target as HTMLElement).closest("a");
      if (!anchor) return;
      e.preventDefault();
      const href = anchor.getAttribute("href") ?? "";
      if (EXTERNAL_HREF.test(href)) void openUrl(href);
    }
    el.addEventListener("click", onClick);
    return () => el.removeEventListener("click", onClick);
  }, []);

  return (
    <div
      ref={ref}
      className={clsx(
        styles.body,
        "markdown-body",
        compact && styles.compact,
        className,
      )}
    />
  );
}
