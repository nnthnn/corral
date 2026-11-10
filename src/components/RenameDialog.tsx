import { type FormEvent, useEffect, useRef, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { loadAll } from "../lib/data";
import { invoke } from "../lib/tauri";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { worktreeTitle } from "../utils";
import { Button } from "./Button";

function close(): void {
  useStore.setState({ renameTarget: null });
}

export function RenameDialog() {
  const target = useStore((s) => s.renameTarget);
  const [wt, setWt] = useState<Worktree | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const open = target !== null;
  const ref = useDialogRef(open);

  useEffect(() => {
    if (!target) return;
    setWt(target);
    setName(worktreeTitle(target));
    setError(null);
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [target]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!wt) return;
    const newName = name.trim();
    if (!newName) return;
    try {
      await invoke("rename_worktree", {
        project: wt.project,
        feature: wt.feature,
        newName,
      });
      close();
      await loadAll();
    } catch (err) {
      setError(String(err));
    }
  }

  if (!wt) return <dialog ref={ref} onClose={close} />;

  return (
    <dialog ref={ref} onClose={close}>
      <form onSubmit={handleSubmit}>
        <h2>Rename worktree</h2>
        <label>
          Name
          <input
            ref={inputRef}
            name="name"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <p className="hint">
          Corral turns this into a kebab-case branch name (currently{" "}
          <code>{wt.branch}</code>) — the worktree's folder and any running
          session don't even notice.
        </p>
        {error && <p className="error">{error}</p>}
        <menu>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button type="submit" variant="primary">
            Rename
          </Button>
        </menu>
      </form>
    </dialog>
  );
}
