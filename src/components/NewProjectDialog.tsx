import { open } from "@tauri-apps/plugin-dialog";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { loadAll } from "../lib/data";
import { invoke } from "../lib/tauri";
import { showError } from "../lib/toasts";
import { createWorktree } from "../lib/worktrees";
import { useStore } from "../store";
import { wtId } from "../utils";
import { Button } from "./Button";

function close(): void {
  useStore.setState({ newProjectOpen: false });
}

/** "Start a brand-new project": names a folder, has corral create it and
 * initialize it as a git repo with one empty commit, then opens its first
 * worktree and starts the agent there. The repo starts empty on purpose — the
 * agent does all the scaffolding in a normal worktree, so Changes/Diff/Merge/PR
 * work from the very first session instead of needing a special
 * "not-a-repo-yet" state. */
export function NewProjectDialog() {
  const isOpen = useStore((s) => s.newProjectOpen);
  const ref = useDialogRef(isOpen);
  const [name, setName] = useState("");
  const [parent, setParent] = useState("");
  const [brief, setBrief] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const nameRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    setName("");
    setParent("");
    setBrief("");
    setError(null);
    setSubmitting(false);
    requestAnimationFrame(() => nameRef.current?.focus());
  }, [isOpen]);

  async function pickParent() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Where should the project live?",
    });
    if (selected && !Array.isArray(selected)) setParent(selected);
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const project = name.trim();
    if (!project || submitting) return;
    setSubmitting(true);
    try {
      await invoke("create_project", { parent: parent.trim(), name: project });
      close();
      // Mirrors `addProject`: register first, then build the first pen and
      // start its agent. A failure here must not leave the new project
      // invisible in the sidebar, so reload regardless.
      const wt = await createWorktree(project).catch(async (err) => {
        showError(
          `Project created, but couldn't build the first pen. (${err})`,
        );
        await loadAll();
        return null;
      });
      const text = brief.trim();
      if (wt && text) {
        // Lands in the agent's input for review — the user presses Enter.
        useStore.getState().sessionMgr?.prefillWhenReady(wtId(wt), text);
      }
    } catch (err) {
      setError(String(err));
      setSubmitting(false);
    }
  }

  return (
    <dialog id="new-project-dialog" ref={ref} onClose={close}>
      <form onSubmit={handleSubmit}>
        <h2>New project</h2>
        <label>
          Name
          <input
            ref={nameRef}
            name="name"
            required
            value={name}
            placeholder="my-app"
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label>
          Location
          <div className="flex gap-2">
            <input
              name="parent"
              value={parent}
              placeholder="~/source (default)"
              onChange={(e) => setParent(e.target.value)}
            />
            <Button
              type="button"
              variant="ghost"
              onClick={() => void pickParent()}
            >
              Choose…
            </Button>
          </div>
        </label>
        <label>
          What are you building? (optional — pasted into the first agent's
          input, not sent)
          <textarea
            name="brief"
            rows={4}
            value={brief}
            placeholder="e.g. A local-first notes app with a Tauri shell and a Rust core"
            onChange={(e) => setBrief(e.target.value)}
          />
        </label>
        <p className="hint">
          Corral creates the folder and initializes it as a git repo with one
          empty commit, then opens its first worktree so an agent can scaffold
          the project there.
        </p>
        {error && <p className="error">{error}</p>}
        <menu>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button
            type="submit"
            variant="primary"
            disabled={submitting || !name.trim()}
          >
            {submitting ? "Creating…" : "Create"}
          </Button>
        </menu>
      </form>
    </dialog>
  );
}
