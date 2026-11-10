import clsx from "clsx";
import {
  ChevronRight,
  File,
  FileArchive,
  FileAudio,
  FileCode,
  FileCog,
  FileImage,
  FileJson,
  FileLock,
  FileSpreadsheet,
  FileText,
  FileVideo,
  Folder,
  type LucideIcon,
} from "lucide-react";
import { dirKey, loadDir, openFile } from "../lib/files";
import { currentWt, useStore } from "../store";
import type { FileEntry, Worktree } from "../types";

/** Extension → icon, for a quick visual scan of a directory listing. Falls
 * back to the generic `File` icon for anything not listed here — this is a
 * cosmetic aid, not a real mimetype sniff, so it only needs to cover common
 * extensions rather than be exhaustive. */
const ICON_BY_EXTENSION: Record<string, LucideIcon> = {
  ts: FileCode,
  tsx: FileCode,
  js: FileCode,
  jsx: FileCode,
  mjs: FileCode,
  cjs: FileCode,
  rs: FileCode,
  py: FileCode,
  go: FileCode,
  rb: FileCode,
  java: FileCode,
  c: FileCode,
  h: FileCode,
  cpp: FileCode,
  hpp: FileCode,
  sh: FileCode,
  fish: FileCode,
  css: FileCode,
  html: FileCode,
  json: FileJson,
  json5: FileJson,
  md: FileText,
  mdx: FileText,
  txt: FileText,
  png: FileImage,
  jpg: FileImage,
  jpeg: FileImage,
  gif: FileImage,
  svg: FileImage,
  webp: FileImage,
  ico: FileImage,
  mp3: FileAudio,
  wav: FileAudio,
  flac: FileAudio,
  ogg: FileAudio,
  mp4: FileVideo,
  mov: FileVideo,
  webm: FileVideo,
  avi: FileVideo,
  zip: FileArchive,
  tar: FileArchive,
  gz: FileArchive,
  rar: FileArchive,
  "7z": FileArchive,
  csv: FileSpreadsheet,
  xlsx: FileSpreadsheet,
  xls: FileSpreadsheet,
  toml: FileCog,
  yaml: FileCog,
  yml: FileCog,
  ini: FileCog,
  conf: FileCog,
  lock: FileLock,
  pem: FileLock,
  key: FileLock,
};

function iconForFile(name: string): LucideIcon {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return File;
  const ext = name.slice(dot + 1).toLowerCase();
  return ICON_BY_EXTENSION[ext] ?? File;
}

function Row({
  wt,
  entry,
  depth,
}: {
  wt: Worktree;
  entry: FileEntry;
  depth: number;
}) {
  const key = dirKey(wt, entry.path);
  const expanded = useStore((s) => s.expandedDirs.has(key));
  const active = useStore((s) => s.fileTab?.path === entry.path);
  const Icon = entry.is_dir ? Folder : iconForFile(entry.name);

  function handleActivate() {
    if (!entry.is_dir) {
      openFile(wt, entry.path);
      return;
    }
    const expandedDirs = new Set(useStore.getState().expandedDirs);
    if (expandedDirs.has(key)) {
      expandedDirs.delete(key);
    } else {
      expandedDirs.add(key);
      loadDir(wt, entry.path);
    }
    useStore.setState({ expandedDirs });
  }

  return (
    <>
      {/* biome-ignore lint/a11y/useSemanticElements: a <button> can't host
       * the nested chevron/icon/name layout without fighting its own
       * default flex/centering behavior — matches ChangesPane's `.change`
       * row, which makes the same call. */}
      <div
        className={clsx(
          "file-row w-full flex items-center gap-1.5 py-1 px-1.5 rounded-md hover:bg-surface-2 cursor-pointer font-mono text-caption",
          active && "bg-surface-2",
        )}
        style={{ paddingLeft: `${6 + depth * 14}px` }}
        role="button"
        tabIndex={0}
        onClick={handleActivate}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            handleActivate();
          }
        }}
      >
        {entry.is_dir ? (
          <ChevronRight
            size={12}
            className={clsx(
              "flex-none text-muted transition-transform duration-100",
              expanded && "rotate-90",
            )}
          />
        ) : (
          <span className="w-3 flex-none" />
        )}
        <Icon size={13} className="flex-none text-muted" />
        <span
          className={clsx(
            "flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap",
            active ? "text-text" : "text-text/70",
          )}
        >
          {entry.name}
        </span>
      </div>
      {entry.is_dir && expanded && (
        <DirContents wt={wt} path={entry.path} depth={depth + 1} />
      )}
    </>
  );
}

function DirContents({
  wt,
  path,
  depth,
}: {
  wt: Worktree;
  path: string;
  depth: number;
}) {
  const entries = useStore((s) => s.fileTree.get(dirKey(wt, path)));
  if (!entries) {
    return (
      <div
        className="text-muted text-caption py-1"
        style={{ paddingLeft: `${6 + depth * 14 + 18}px` }}
      >
        Loading…
      </div>
    );
  }
  return (
    <>
      {entries.map((entry) => (
        <Row key={entry.path} wt={wt} entry={entry} depth={depth} />
      ))}
    </>
  );
}

export function FileTree() {
  const wt = useStore(currentWt);
  if (!wt) return null;
  return <DirContents wt={wt} path="" depth={0} />;
}
