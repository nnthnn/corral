import { useStore } from "../store";
import type { LinearIssue } from "../types";
import { invoke } from "./tauri";
import { showError, showSuccess } from "./toasts";

/** Window event fired after the Linear API key is added, replaced, or removed,
 * so the open dialog drops a result list that belongs to the old key. A DOM
 * event rather than store state because the listener (the dialog) may be
 * closed, and because the key's value isn't something the store should hold.
 */
export const LINEAR_KEY_SAVED_EVENT = "corral:linear-key-saved";

export function announceLinearKeySaved(): void {
  window.dispatchEvent(new Event(LINEAR_KEY_SAVED_EVENT));
}

/** Render a ticket as a self-contained markdown document. Written to a temp
 * file and referenced by path (like a failed CI log or PR comments — see
 * `save_text_attachment`) so a long ticket body doesn't flood the terminal. */
export function formatIssueAttachment(issue: LinearIssue): string {
  const meta: string[] = [];
  if (issue.url) meta.push(`- **URL:** ${issue.url}`);
  if (issue.state) meta.push(`- **State:** ${issue.state.name}`);
  if (issue.team) meta.push(`- **Team:** ${issue.team}`);
  if (issue.project) meta.push(`- **Project:** ${issue.project.name}`);
  if (issue.assignee) meta.push(`- **Assignee:** ${issue.assignee}`);
  if (issue.priority) meta.push(`- **Priority:** ${issue.priority}`);
  if (issue.labels.length)
    meta.push(`- **Labels:** ${issue.labels.join(", ")}`);

  return [
    `# ${issue.identifier}: ${issue.title}`,
    "",
    ...meta,
    "",
    "## Description",
    "",
    issue.description?.trim() || "(no description)",
    "",
  ].join("\n");
}

/** The prompt inserted into the agent's input. Ends with a space (not a
 * newline) so the user can immediately type their own instruction before
 * submitting — `attachIssueToAgent` deliberately does not press Enter. */
export function issueAttachmentPrompt(
  issue: LinearIssue,
  path: string,
): string {
  const state = issue.state ? ` (${issue.state.name})` : "";
  return `Linear ticket ${issue.identifier}: "${issue.title}"${state}. Full details are attached at "${path}" — please read it. `;
}

/** Save the ticket body to a temp file and insert a reference into the active
 * agent session, without submitting it. Returns false (after telling the user
 * why) when no session is attached. */
export async function attachIssueToAgent(issue: LinearIssue): Promise<boolean> {
  const text = formatIssueAttachment(issue);
  let path: string;
  try {
    path = await invoke("save_text_attachment", { text });
  } catch (err) {
    showError(`Couldn't write the ticket attachment: ${err}`);
    return false;
  }

  // Switch to Agent first so the terminal is visible (and focused) when the
  // prompt lands in its input.
  useStore.setState({ activeTab: "agent" });
  const inserted =
    useStore
      .getState()
      .sessionMgr?.insertToActive(issueAttachmentPrompt(issue, path)) ?? false;
  if (!inserted) {
    showError("Start the agent first, then attach a ticket.");
    return false;
  }
  showSuccess(`Attached ${issue.identifier} to the agent.`);
  return true;
}
