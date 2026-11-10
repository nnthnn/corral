/** Syntax highlighting for the diff viewer, backed by highlight.js's
 * tree-shakeable core plus the handful of languages a coding-agent repo
 * actually produces. Pure-JS and synchronous (no WASM/async grammar loading),
 * which matters here: the app's release build has a documented esbuild
 * minifier-mangling gotcha with heavier ESM/WASM dependencies, and a blank
 * diff is far worse than no highlighting. */

import type { LanguageFn } from "highlight.js";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import plaintext from "highlight.js/lib/languages/plaintext";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scss from "highlight.js/lib/languages/scss";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const LANGUAGES: [string, LanguageFn][] = [
  ["bash", bash],
  ["c", c],
  ["cpp", cpp],
  ["css", css],
  ["dockerfile", dockerfile],
  ["go", go],
  ["ini", ini],
  ["java", java],
  ["javascript", javascript],
  ["json", json],
  ["makefile", makefile],
  ["markdown", markdown],
  ["php", php],
  ["plaintext", plaintext],
  ["python", python],
  ["ruby", ruby],
  ["rust", rust],
  ["scss", scss],
  ["sql", sql],
  ["typescript", typescript],
  ["xml", xml],
  ["yaml", yaml],
];

for (const [name, fn] of LANGUAGES) hljs.registerLanguage(name, fn);

/** Languages registered above, in the same order — for tests and any future
 * "which languages do we support" surface. */
export const REGISTERED_LANGUAGES: readonly string[] = LANGUAGES.map(
  ([name]) => name,
);

/** Highlights one line of code, returning HTML with `<span class="hljs-…">`
 * tokens. The input is escaped by highlight.js, so the result is safe to
 * render via `dangerouslySetInnerHTML`. `language` should come from
 * `languageForPath`; unknown/`null` values fall back to plain (escaped) text.
 * Never throws — a grammar hiccup degrades to escaped plain text. */
export function highlightCode(code: string, language: string | null): string {
  const name = language && hljs.getLanguage(language) ? language : "plaintext";
  try {
    return hljs.highlight(code, { language: name, ignoreIllegals: true }).value;
  } catch {
    return hljs.highlight(code, { language: "plaintext" }).value;
  }
}
