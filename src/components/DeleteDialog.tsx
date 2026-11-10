import { Loader2 } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { removeWorktree } from "../lib/actions";
import { showError } from "../lib/toasts";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { shortenPath } from "../utils";
import { Button } from "./Button";

function close(): void {
  useStore.setState({ deleteTarget: null });
}

export function DeleteDialog() {
  const target = useStore((s) => s.deleteTarget);
  const homeDir = useStore((s) => s.homeDir);
  const deleteBranchDefault = useStore((s) => s.settings.delete_branch_default);
  const [wt, setWt] = useState<Worktree | null>(null);
  const [deleteBranch, setDeleteBranch] = useState(deleteBranchDefault);
  const [removing, setRemoving] = useState(false);
  const open = target !== null;
  const ref = useDialogRef(open);

  useEffect(() => {
    if (!target) return;
    setWt(target);
    setDeleteBranch(deleteBranchDefault);
  }, [target, deleteBranchDefault]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!wt || removing) return;
    setRemoving(true);
    try {
      await removeWorktree(wt, deleteBranch);
    } catch (err) {
      showError(`Couldn't clear it out: ${err}`);
    } finally {
      setRemoving(false);
    }
  }

  if (!wt) return <dialog ref={ref} onClose={close} />;

  const st = wt.status;
  const dirty = st ? st.staged + st.modified + st.untracked : 0;
  const warnings: string[] = [];
  if (dirty > 0) {
    warnings.push(
      `${dirty} uncommitted change${dirty === 1 ? "" : "s"} will be lost.`,
    );
  }
  if (st && st.ahead > 0) {
    warnings.push(
      `${st.ahead} commit${st.ahead === 1 ? "" : "s"} not merged upstream.`,
    );
  }

  return (
    <dialog
      ref={ref}
      onClose={close}
      onCancel={(e) => {
        // Escape fires `cancel` before `close` — block it mid-removal so
        // the dialog can't disappear while the backend call is in flight.
        if (removing) e.preventDefault();
      }}
    >
      <form onSubmit={handleSubmit}>
        <h2>Remove worktree?</h2>
        <p className="hint">This one's not coming back.</p>
        <p className="hint">
          {wt.project}/{wt.feature}
          <br />
          <code className="detail-path text-muted text-code">
            {shortenPath(wt.path, homeDir)}
          </code>
        </p>
        {warnings.length > 0 && <p className="warn">{warnings.join(" ")}</p>}
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={deleteBranch}
            disabled={removing}
            onChange={(e) => setDeleteBranch(e.target.checked)}
          />
          <span className="whitespace-nowrap">Also delete branch</span>
        </label>
        <p className="hint">
          <code className="whitespace-normal break-all">{wt.branch}</code>
        </p>
        <menu>
          <Button variant="ghost" disabled={removing} onClick={close}>
            Cancel
          </Button>
          <Button type="submit" variant="danger" disabled={removing}>
            {removing ? (
              <>
                <Loader2 size={14} className="animate-spin mr-1.5" />
                Removing…
              </>
            ) : (
              "Remove"
            )}
          </Button>
        </menu>
      </form>
    </dialog>
  );
}
