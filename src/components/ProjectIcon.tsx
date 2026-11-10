import { ChevronDown, ChevronRight, FolderGit2 } from "lucide-react";
import { useStore } from "../store";

/** Auto-detected repo icon for a project group header, falling back to a
 * default glyph when nothing was found (or the lookup hasn't resolved yet —
 * see `ensureProjectIcons` in `lib/data.ts`). On hover of the group header
 * (`.wt-group-head`'s `group`), swaps to a chevron hinting that the row
 * collapses/expands on click — direction matches the row's current state. */
export function ProjectIcon({
  project,
  collapsed,
}: {
  project: string;
  collapsed: boolean;
}) {
  const iconUrl = useStore((s) => s.projectIcons.get(project));
  const Chevron = collapsed ? ChevronRight : ChevronDown;

  return (
    <span className="relative size-5.5 flex-none grid">
      <span className="col-start-1 row-start-1 group-hover:opacity-0">
        {typeof iconUrl === "string" ? (
          <img
            src={iconUrl}
            alt=""
            className="w-5.5 h-5.5 rounded-sm object-cover"
          />
        ) : (
          <FolderGit2 size={19} className="text-muted" aria-hidden />
        )}
      </span>
      <span className="col-start-1 row-start-1 flex items-center justify-center opacity-0 group-hover:opacity-100">
        <Chevron size={19} className="text-muted" aria-hidden />
      </span>
    </span>
  );
}
