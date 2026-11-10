/** Pure helpers for rendering a unified diff — line classification, splitting,
 * and language detection. Kept free of React and highlight.js so the logic is
 * trivially unit-testable; the highlight.js wiring lives in `highlight.ts`. */

export type DiffLineKind = "meta" | "hunk" | "add" | "del" | "ctx";

export interface DiffLine {
  /** Full raw line text, including git's leading `+`/`-`/space prefix. */
  text: string;
  kind: DiffLineKind;
}

/** Classifies one raw unified-diff line. Order matters: `---`/`+++` must be
 * checked before the single `+`/`-` cases, and `@@`/`diff `/`index ` before
 * treating a leading `-` as a deletion. */
export function classifyDiffLine(line: string): DiffLineKind {
  // `\ No newline at end of file` — render muted, not as code.
  if (line.startsWith("\\")) return "meta";
  if (line.startsWith("+++") || line.startsWith("---")) return "meta";
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("diff ") || line.startsWith("index ")) return "meta";
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  return "ctx";
}

/** Splits a unified diff into classified lines. */
export function splitDiff(diff: string): DiffLine[] {
  return diff
    .split("\n")
    .map((text) => ({ text, kind: classifyDiffLine(text) }));
}

const EXT_LANGUAGE: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  rs: "rust",
  py: "python",
  pyi: "python",
  json: "json",
  jsonc: "json",
  css: "css",
  scss: "scss",
  less: "css",
  sass: "scss",
  html: "xml",
  htm: "xml",
  xml: "xml",
  svg: "xml",
  md: "markdown",
  markdown: "markdown",
  yaml: "yaml",
  yml: "yaml",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  fish: "bash",
  go: "go",
  sql: "sql",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  java: "java",
  rb: "ruby",
  php: "php",
  ini: "ini",
  conf: "ini",
  cfg: "ini",
  toml: "ini",
};

/** A few files identified by name rather than extension. */
const BASENAME_LANGUAGE: Record<string, string> = {
  Dockerfile: "dockerfile",
  Makefile: "makefile",
};

/** Maps a repo-relative path to a highlight.js language name, or `null` when
 * none is known (callers fall back to plain text). */
export function languageForPath(path: string): string | null {
  const slash = path.lastIndexOf("/");
  const basename = slash === -1 ? path : path.slice(slash + 1);
  const known = BASENAME_LANGUAGE[basename];
  if (known) return known;

  const dot = basename.lastIndexOf(".");
  if (dot === -1 || dot === basename.length - 1) return null;
  return EXT_LANGUAGE[basename.slice(dot + 1).toLowerCase()] ?? null;
}
