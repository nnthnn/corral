/** Wraps the first case-insensitive match of `needle` (the active filter) in
 * a <mark>, so the sidebar can show why a row matched. `needle` is expected
 * pre-lowercased by the caller. JSX-returning counterpart to `utils.ts`'s
 * `highlightMatch` (which returns an HTML string for the old innerHTML
 * rendering, locked in by its own tests) — same logic, different output
 * shape. */
export function Highlight({ text, needle }: { text: string; needle: string }) {
  if (!needle) return <>{text}</>;
  const idx = text.toLowerCase().indexOf(needle);
  if (idx === -1) return <>{text}</>;
  const before = text.slice(0, idx);
  const match = text.slice(idx, idx + needle.length);
  const after = text.slice(idx + needle.length);
  return (
    <>
      {before}
      <mark className="match">{match}</mark>
      {after}
    </>
  );
}
