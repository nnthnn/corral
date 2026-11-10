import { useStore } from "../store";
import type { PrInfo, Worktree } from "../types";
import { orderProjects, wtId } from "../utils";
import { selectWorktree } from "./actions";
import { startAgent } from "./agent-actions";
import { loadAll } from "./data";
import { toggleSetting } from "./settings";
import { invoke } from "./tauri";
import { showError, showSuccess } from "./toasts";

/** Create a worktree with an auto-picked placeholder feature name — used by
 * the sidebar's per-group "+" button, which already knows the project and
 * skips the "New worktree" dialog entirely, and by `addProject()`
 * (`./projects.ts`) to auto-create a new project's first worktree right
 * after it's registered. */
export async function createWorktree(project: string): Promise<Worktree> {
  const wt = await invoke("create_worktree", {
    project,
    feature: null,
  });
  selectWorktree(wtId(wt));
  await loadAll();
  void startAgent(wt);
  showSuccess("New pen's ready.");
  return wt;
}

/** Drives the user to the sidebar's existing "+" button instead of
 * duplicating worktree creation — used by the homepage empty state's "New
 * worktree" card. Expands the sidebar if it's collapsed, then sets
 * `worktreeNudgeTarget` to the first project in sidebar order; `WorktreeTree`
 * does the actual scroll-to/flash and clears the field afterward. */
export function focusFirstProjectCreate(): void {
  const s = useStore.getState();
  if (s.settings.sidebar_collapsed) {
    toggleSetting("sidebar_collapsed", false);
  }
  const [first] = orderProjects(s.projects, s.settings.project_order);
  if (!first) return;
  useStore.setState({ worktreeNudgeTarget: first });
}

export async function openDeleteDialog(wt: Worktree): Promise<void> {
  const id = wtId(wt);
  if (useStore.getState().sessions.get(id)?.running) {
    showError("Stop the agent before removing this worktree.");
    return;
  }
  // wt.status can be stale if an agent committed/merged via its own PTY
  // shell since the last loadAll() — refresh before showing the warning.
  await loadAll();
  const fresh = useStore.getState().worktrees.find((w) => wtId(w) === id) ?? wt;
  useStore.setState({ deleteTarget: fresh });
}

export function openRenameDialog(wt: Worktree): void {
  useStore.setState({ renameTarget: wt });
}

/** Saves a worktree's note, optimistically updating the local store so the
 * sidebar reflects it immediately rather than waiting on a full reload. */
export async function setWorktreeNote(
  wt: Worktree,
  note: string,
): Promise<void> {
  const id = wtId(wt);
  const trimmed = note.trim();
  const worktrees = useStore
    .getState()
    .worktrees.map((w) => (wtId(w) === id ? { ...w, note: trimmed } : w));
  useStore.setState({ worktrees });
  try {
    await invoke("set_worktree_note", {
      project: wt.project,
      feature: wt.feature,
      note: trimmed,
    });
  } catch (err) {
    showError(`Couldn't save note: ${err}`);
  }
}

export function openProjectSettingsDialog(project: string): void {
  useStore.setState({ projectSettingsTarget: project });
}

export function toggleGroup(project: string): void {
  const collapsedGroups = new Set(useStore.getState().collapsedGroups);
  if (collapsedGroups.has(project)) collapsedGroups.delete(project);
  else collapsedGroups.add(project);
  useStore.setState({ collapsedGroups });
  toggleSetting("collapsed_groups", [...collapsedGroups]);
}

/** Persists a full drag-or-keyboard reorder of the sidebar's project list —
 * `order` should already be the complete, freshly-computed project order
 * (see `orderProjects`/`moveProject`/`reorderProjects` in utils.ts). */
export function setProjectOrder(order: string[]): void {
  toggleSetting("project_order", order);
}

export async function mergeWorktree(wt: Worktree): Promise<void> {
  const id = wtId(wt);
  // The button disables itself once `mergingIds` includes `id`, but that's
  // a render away — a click landing before the DOM updates (e.g. a fast
  // double-click) would otherwise re-enter this function and fire a second
  // concurrent merge.
  if (useStore.getState().mergingIds.has(id)) return;
  const mergingIds = new Set(useStore.getState().mergingIds);
  mergingIds.add(id);
  useStore.setState({ mergingIds });
  try {
    const result = await invoke("merge_worktree", {
      project: wt.project,
      feature: wt.feature,
    });
    switch (result.outcome) {
      case "merged": {
        showSuccess(
          result.pushed
            ? "Merged into main and pushed to origin."
            : "Merged into main.",
        );
        const justMergedIds = new Set(useStore.getState().justMergedIds);
        justMergedIds.add(id);
        useStore.setState({ justMergedIds });
        break;
      }
      case "conflict":
        showError(
          "Two of them hit the same ground. Resolve it in the terminal, then merge again.",
        );
        break;
      case "dirty":
        showError("Commit or stash changes before merging.");
        break;
      case "no_base_branch":
        showError("Couldn't find a base branch to merge into.");
        break;
      case "base_diverged":
        showError(
          `The main repo's "${result.base}" branch has drifted from its remote. Reconcile it there (e.g. git pull --rebase), then merge again.`,
        );
        break;
      case "merged_push_failed":
        showError(
          `Merged into main locally, but pushing to origin failed. See what's blocking it. (${result.error})`,
        );
        break;
      case "not_on_base_branch":
        showError(
          `The main repo is on branch "${result.current}", not "${result.base}". Check out ${result.base} there first.`,
        );
        break;
    }
  } catch (err) {
    showError(`Git pushed back. See what's blocking it. (${err})`);
  } finally {
    const stillMerging = new Set(useStore.getState().mergingIds);
    stillMerging.delete(id);
    useStore.setState({ mergingIds: stillMerging });
    await loadAll();
  }
}

export async function rebaseWorktree(wt: Worktree): Promise<void> {
  const id = wtId(wt);
  if (useStore.getState().rebasingIds.has(id)) return;
  const rebasingIds = new Set(useStore.getState().rebasingIds);
  rebasingIds.add(id);
  useStore.setState({ rebasingIds });
  try {
    const result = await invoke("rebase_worktree", {
      project: wt.project,
      feature: wt.feature,
    });
    switch (result.outcome) {
      case "rebased":
        showSuccess("Rebased onto the base branch.");
        break;
      case "conflict":
        showError(
          "Rebase hit a conflict. Resolve it manually, then try again.",
        );
        break;
      case "dirty":
        showError("Commit or stash changes before rebasing.");
        break;
      case "no_base_branch":
        showError("Couldn't find a base branch to rebase onto.");
        break;
    }
  } catch (err) {
    showError(`Git pushed back. See what's blocking it. (${err})`);
  } finally {
    const stillRebasing = new Set(useStore.getState().rebasingIds);
    stillRebasing.delete(id);
    useStore.setState({ rebasingIds: stillRebasing });
    await loadAll();
  }
}

function rebaseInstructionsMarkdown(wt: Worktree, base: string): string {
  return `# Rebase \`${wt.branch}\` onto \`${base}\`

This branch conflicts with \`${base}\` (its base branch) — a plain merge would
fail, so it needs a rebase with the conflicts resolved by hand.

1. \`git fetch origin ${base}\`
2. \`git rebase origin/${base}\`
3. For each conflict git stops on: open the listed files, resolve the
   \`<<<<<<<\`/\`=======\`/\`>>>>>>>\` markers, then \`git add\` the resolved files
   and run \`git rebase --continue\`. Repeat until the rebase finishes.
4. If this branch already has commits pushed to a remote (check with
   \`git status -sb\` — it'll show an \`origin/${wt.branch}\` upstream), finish
   with \`git push --force-with-lease\` to update it safely. A rebase rewrites
   commits the remote already has, so a plain \`git push\` will be rejected.

Don't use \`git merge\` — a rebase keeps history linear, matching how this
branch's other history was built.`;
}

/** Asks the active agent session to perform a rebase, instead of corral
 * doing it directly — unlike `rebaseWorktree` (which shells out to `git
 * rebase` and auto-aborts on the first conflict, leaving the worktree
 * untouched), this lets Claude actually work through conflicts. Only wired
 * up for `RebaseButton`'s `"chat"` mode (`ChangesPane`, always the active
 * worktree) — pasting into "the active session" isn't meaningful for a
 * worktree that isn't currently selected. Writes the how-to as a temp-file
 * attachment (see `addCheckToChat`'s doc comment for why: same reasoning
 * applies to a multi-step how-to as to a long log) and falls back to an
 * inline summary if that write fails. */
export async function askClaudeToRebase(
  wt: Worktree,
  base: string,
): Promise<void> {
  const instructions = rebaseInstructionsMarkdown(wt, base);
  const path = await invoke("save_text_attachment", {
    text: instructions,
  }).catch(() => null);
  const prompt = path
    ? `This branch has conflicts with its base branch (\`${base}\`). Please rebase it, resolving any conflicts that come up. See "${path}" for step-by-step instructions.`
    : `This branch has conflicts with its base branch (\`${base}\`). Please rebase onto it (\`git rebase origin/${base}\`), resolving any conflicts that come up, then force-push with lease if it already has a remote.`;
  useStore.setState({ activeTab: "agent" });
  useStore.getState().sessionMgr?.pasteToActive(prompt);
}

function pullRequestInstructionsMarkdown(wt: Worktree): string {
  const status = wt.status;
  return `# Prepare a pull request for \`${wt.branch}\`

Worktree: \`${wt.path}\`
Base branch: detect the appropriate base branch before rebasing.

## Instructions

Prepare this branch for a pull request. Inspect the complete diff and current
repository state before acting.

- If there are uncommitted files, explain what they are and ask the user whether
  they should be included. Do not commit them without confirmation.
- Fetch \`origin\`, detect the base branch, and rebase onto its latest remote ref
  (\`origin/<base>\` when available), resolving conflicts carefully rather than
  aborting automatically. Do not merge the base branch into this branch.
- Run appropriate tests and checks after resolving conflicts.
- Push the branch with \`git push --force-with-lease\` when a rebase requires it.
- Create or update the GitHub pull request with \`gh\`, generating a concise,
  specific title and a Markdown description with Summary, Changes, and Testing
  sections.
- Do not merge the pull request.
- Report the actions taken, any decisions requested from the user, and the PR
  URL when available.

## Status snapshot

- Commits ahead of base: ${status?.commits_ahead_of_base ?? "unknown"}
- Staged files: ${status?.staged ?? "unknown"}
- Modified files: ${status?.modified ?? "unknown"}
- Untracked files: ${status?.untracked ?? "unknown"}
- Conflicts: ${status?.conflicts ?? "unknown"}
`;
}

let nextPrPreparationToken = 0;
const PR_PREPARATION_TIMEOUT_MS = 30 * 60 * 1000;

function clearPrPreparation(id: string, token: number): void {
  const current = useStore.getState().preparingPr.get(id);
  if (current?.token !== token) return;
  const preparingPr = new Map(useStore.getState().preparingPr);
  preparingPr.delete(id);
  useStore.setState({ preparingPr });
}

/** Hands PR preparation to the active agent instead of duplicating an AI
 * integration in Corral's PR shell command. */
export async function askAgentToPreparePullRequest(
  wt: Worktree,
): Promise<void> {
  const id = wtId(wt);
  const initial = useStore.getState();
  if (initial.preparingPr.has(id)) return;
  const session = initial.sessions.get(id);
  if (initial.selectedId !== id || !session?.running) {
    showError("Start the agent for this worktree before preparing a PR.");
    return;
  }
  if (!session.waiting) {
    showError(
      "Wait for the agent to finish its current task before preparing a PR.",
    );
    return;
  }
  const token = ++nextPrPreparationToken;
  const preparingPr = new Map(initial.preparingPr);
  preparingPr.set(id, {
    token,
    kind: initial.prInfoByWtId.get(id)?.state === "open" ? "update" : "create",
    initialAhead: wt.status?.ahead ?? 0,
    waits: 0,
  });
  useStore.setState({ preparingPr });
  window.setTimeout(
    () => clearPrPreparation(id, token),
    PR_PREPARATION_TIMEOUT_MS,
  );

  const path = await invoke("save_text_attachment", {
    text: pullRequestInstructionsMarkdown(wt),
  }).catch(() => null);
  const prompt = path
    ? `Prepare this branch for a pull request using the instructions in "${path}". Ask me before including any uncommitted files.`
    : "Prepare this branch for a pull request. Inspect the diff, ask before including uncommitted files, rebase and resolve conflicts, run checks, push safely, then create or update the PR without merging it.";
  const current = useStore.getState();
  if (current.selectedId !== id || !current.sessions.get(id)?.running) {
    clearPrPreparation(id, token);
    showError(
      "PR preparation was cancelled because the active worktree changed.",
    );
    return;
  }
  useStore.setState({ activeTab: "agent" });
  if (!(await current.sessionMgr?.submitTo(id, prompt))) {
    clearPrPreparation(id, token);
    showError("Couldn't send PR instructions to the active agent.");
    return;
  }
  showSuccess("Asked the agent to prepare this pull request.");
}

/** Fetch and cache PR state for `wt`, keyed by `wtId`.
 * Only meaningful for a worktree with a remote — callers gate on
 * `wt.status?.has_remote` before calling this (see `selectWorktree`). */
export function fetchPrStatus(
  wt: Worktree,
): Promise<PrInfo | null | undefined> {
  const id = wtId(wt);
  return invoke("pr_status", { project: wt.project, feature: wt.feature })
    .then((info: PrInfo | null) => {
      const prInfoByWtId = new Map(useStore.getState().prInfoByWtId);
      prInfoByWtId.set(id, info);
      useStore.setState({ prInfoByWtId });
      return info;
    })
    .catch((err) => {
      console.error("fetchPrStatus failed:", err);
      return undefined;
    });
}
