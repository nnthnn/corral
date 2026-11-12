/** Markdown → sanitized HTML for the File preview's rendered view.
 *
 * `marked` handles the GFM parsing (tables, task lists, fenced code); every
 * produced string still goes through DOMPurify before it reaches the DOM,
 * because a repo file is untrusted input and raw HTML in markdown would
 * otherwise be a script-injection vector into the webview. Fenced code is
 * tokenized with the same highlight.js core the source view uses. */

import DOMPurify from "dompurify";
import { Marked } from "marked";
import type { Worktree } from "../types";
import { imageDataUrl, isSvgPath, svgDataUrl } from "./files";
import { highlightCode } from "./highlight";
import { invoke } from "./tauri";

const marked = new Marked({ gfm: true, breaks: false });

marked.use({
  renderer: {
    code({ text, lang }) {
      // Info strings can carry extra metadata ("ts title=foo.ts"); the
      // language is just the first token. `highlightCode` falls back to
      // escaped plain text for anything highlight.js doesn't know.
      const language = lang?.trim().split(/\s+/)[0] || null;
      const cls = language
        ? ` class="language-${language.replace(/[^a-z0-9_-]/gi, "")}"`
        : "";
      return `<pre><code${cls}>${highlightCode(text, language)}</code></pre>`;
    },
  },
});

/** Renders markdown to HTML, sanitized so no script/event-handler/`javascript:`
 * can survive from the source file. Relative `<img src>` values are left
 * untouched here — `resolveMarkdownImages` rewrites those afterwards. */
export function renderMarkdown(md: string): string {
  const html = marked.parse(md) as string;
  return DOMPurify.sanitize(html, { USE_PROFILES: { html: true } });
}

/** True for a `src` we might be able to resolve inside the worktree: a
 * relative path, not an external URL, `data:` URI, protocol-relative URL, or
 * bare fragment. */
export function isResolvableImageSrc(src: string): boolean {
  if (!src) return false;
  if (src.startsWith("//") || src.startsWith("#")) return false;
  return !/^[a-z][a-z0-9+.-]*:/i.test(src);
}

/** Joins a relative markdown path onto `baseDir` (the markdown file's own
 * directory — GFM resolves relative to the document, not the repo root),
 * collapsing `.`/`..`. Returns `null` if the result would escape the worktree,
 * so a `../../etc/passwd` reference is dropped rather than sent to the
 * backend's already-defensive path check. */
export function resolveRelativePath(
  baseDir: string,
  src: string,
): string | null {
  let decoded: string;
  try {
    decoded = decodeURIComponent(src);
  } catch {
    decoded = src;
  }
  // Drop any `?query`/`#fragment` before resolving.
  const clean = decoded.split(/[?#]/)[0];
  const joined = clean.startsWith("/")
    ? clean.slice(1)
    : baseDir
      ? `${baseDir}/${clean}`
      : clean;

  const parts: string[] = [];
  for (const part of joined.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  return parts.join("/");
}

/** Rewrites every worktree-relative `<img>` in `html` to a `data:` URL by
 * reading the file through `read_worktree_file` (which already returns image
 * bytes base64-encoded). Best-effort: anything that fails to resolve — missing
 * file, an oversized image, a non-image extension — is left as-is rather than
 * failing the whole preview. */
export async function resolveMarkdownImages(
  wt: Pick<Worktree, "project" | "feature">,
  markdownPath: string,
  html: string,
): Promise<string> {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const imgs = Array.from(doc.querySelectorAll("img[src]"));
  if (imgs.length === 0) return html;

  const slash = markdownPath.lastIndexOf("/");
  const baseDir = slash === -1 ? "" : markdownPath.slice(0, slash);

  await Promise.all(
    imgs.map(async (img) => {
      const src = img.getAttribute("src") ?? "";
      if (!isResolvableImageSrc(src)) return;
      const path = resolveRelativePath(baseDir, src);
      if (!path) return;
      try {
        const content = await invoke("read_worktree_file", {
          project: wt.project,
          feature: wt.feature,
          path,
        });
        if (content.kind === "image") {
          img.setAttribute("src", imageDataUrl(content.mime, content.data));
        } else if (content.kind === "text" && isSvgPath(path)) {
          // SVG comes back as text (it's a text format — see `read_file`),
          // so it doesn't satisfy `kind === "image"`. Encode the source
          // ourselves, same as the File preview does.
          img.setAttribute("src", svgDataUrl(content.text));
        }
      } catch {
        // Leave the original (broken) src in place.
      }
    }),
  );

  return doc.body.innerHTML;
}
