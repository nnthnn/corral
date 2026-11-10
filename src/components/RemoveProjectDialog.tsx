import { type FormEvent, useEffect, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { loadAll } from "../lib/data";
import { invoke } from "../lib/tauri";
import { showError, showSuccess } from "../lib/toasts";
import { useStore } from "../store";
import { Button } from "./Button";

function close(): void {
  useStore.setState({ removeProjectTarget: null });
}

export function RemoveProjectDialog() {
  const project = useStore((s) => s.removeProjectTarget);
  const sessions = useStore((s) => s.sessions);
  const open = project !== null;
  const ref = useDialogRef(open);
  const [submitting, setSubmitting] = useState(false);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reset-on-project-change
  useEffect(() => {
    setSubmitting(false);
  }, [project]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!project) return;
    setSubmitting(true);
    try {
      await invoke("remove_project", { project });
      close();
      if (useStore.getState().projectSettingsTarget === project) {
        useStore.setState({ projectSettingsTarget: null });
      }
      await loadAll();
      showSuccess("Project removed.");
    } catch (err) {
      showError(`Couldn't remove project: ${err}`);
      setSubmitting(false);
    }
  }

  if (!project) return <dialog ref={ref} onClose={close} />;

  const runningCount = [...sessions.values()].filter(
    (s) => s.project === project && s.running,
  ).length;

  return (
    <dialog ref={ref} onClose={close}>
      <form onSubmit={handleSubmit}>
        <h2>Remove project?</h2>
        <p className="hint">
          <code>{project}</code> disappears from corral. Nothing on disk is
          touched — the repo and its worktrees stay exactly where they are. Add
          it back anytime from Projects.
        </p>
        {runningCount > 0 && (
          <p className="warn">
            {runningCount} running session{runningCount === 1 ? "" : "s"} will
            be stopped.
          </p>
        )}
        <menu>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button type="submit" variant="danger" disabled={submitting}>
            Remove
          </Button>
        </menu>
      </form>
    </dialog>
  );
}
