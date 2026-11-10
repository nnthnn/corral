import { splitRenamedPath } from "../utils";

/** Splits `dir/of/path/file.ts` into a dimmed, independently-truncating
 * directory portion and a full-opacity filename that only gives up space
 * once the directory has already shrunk to nothing — `shrink-[20]` vs. the
 * filename's default `shrink` weights truncation heavily toward the
 * directory first, so the filename itself is only truncated once there's no
 * directory left to sacrifice.
 *
 * For a rename (`oldPath` set), shows `old → new` instead of just the new
 * path: when both live in the same directory, that directory is shown once
 * and only the filenames are struck through/diffed; otherwise falls back to
 * the full old path struck through, then the full new path. */
export function PathLabel({
  path,
  oldPath,
}: {
  path: string;
  oldPath?: string | null;
}) {
  if (oldPath) {
    const split = splitRenamedPath(oldPath, path);
    const [before, oldName, newName] = split
      ? [split.dir, split.oldName, split.newName]
      : ["", oldPath, path];
    return (
      <span className="flex min-w-0 items-baseline">
        {before && (
          <span className="min-w-0 shrink-[20] overflow-hidden text-ellipsis whitespace-nowrap opacity-60">
            {before}
          </span>
        )}
        <span className="min-w-0 shrink-[20] overflow-hidden text-ellipsis whitespace-nowrap text-muted line-through opacity-60">
          {oldName}
        </span>
        <span className="shrink-0 px-1 text-muted opacity-70">→</span>
        <span className="min-w-0 shrink overflow-hidden text-ellipsis whitespace-nowrap">
          {newName}
        </span>
      </span>
    );
  }
  const slash = path.lastIndexOf("/");
  if (slash === -1) {
    return (
      <span className="block overflow-hidden text-ellipsis whitespace-nowrap">
        {path}
      </span>
    );
  }
  return (
    <span className="flex min-w-0 items-baseline">
      <span className="min-w-0 shrink-[20] overflow-hidden text-ellipsis whitespace-nowrap opacity-60">
        {path.slice(0, slash + 1)}
      </span>
      <span className="min-w-0 shrink overflow-hidden text-ellipsis whitespace-nowrap">
        {path.slice(slash + 1)}
      </span>
    </span>
  );
}
