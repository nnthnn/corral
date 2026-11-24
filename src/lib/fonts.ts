import { invoke } from "./tauri";
import { isGenericFontFamily } from "./terminal";

/** Suggestions used when the backend can't enumerate the system's fonts (a
 * scan failure, or a webview running tests). Deliberately short — just
 * enough to keep the picker useful. */
export const FALLBACK_MONOSPACE_FONTS = [
  "JetBrains Mono",
  "Fira Code",
  "monospace",
];

export interface MonospaceFonts {
  /** Candidate families for the terminal font picker, sorted by the backend. */
  families: string[];
  /** Whether `families` came from the backend's scan of this machine, or is
   * [`FALLBACK_MONOSPACE_FONTS`]. Only an enumerated list is authoritative
   * enough to warn that a configured family isn't installed. */
  enumerated: boolean;
}

const FALLBACK: MonospaceFonts = {
  families: FALLBACK_MONOSPACE_FONTS,
  enumerated: false,
};

/** Monospaced font families installed on this machine, for the terminal font
 * picker's `<datalist>` (see `commands::fonts` in the Tauri crate). Always
 * includes the generic `monospace` keyword, and falls back to
 * [`FALLBACK_MONOSPACE_FONTS`] rather than failing — the input stays free
 * text either way, so this is purely advisory. */
export async function monospaceFonts(): Promise<MonospaceFonts> {
  try {
    const families = await invoke("list_monospace_fonts");
    if (!Array.isArray(families) || families.length === 0) return FALLBACK;
    return {
      families: Array.from(new Set([...families, "monospace"])),
      enumerated: true,
    };
  } catch {
    return FALLBACK;
  }
}

/** Split a CSS font stack into trimmed, unquoted family names. */
export function fontStackFamilies(stack: string): string[] {
  return stack
    .split(",")
    .map((family) => family.trim().replace(/^["']|["']$/g, ""))
    .filter(Boolean);
}

/** The first concrete family in `stack` that isn't among `installed`, or
 * `null` when there's nothing worth warning about: a blank stack, one made
 * only of generic keywords, or one where at least one concrete family is
 * installed. Matching is case-insensitive, like the engine's own lookups. */
export function firstUninstalledFamily(
  stack: string,
  installed: string[],
): string | null {
  const concrete = fontStackFamilies(stack).filter(
    (family) => !isGenericFontFamily(family),
  );
  if (concrete.length === 0) return null;
  const installedLower = new Set(installed.map((f) => f.toLowerCase()));
  if (concrete.some((f) => installedLower.has(f.toLowerCase()))) return null;
  return concrete[0];
}
