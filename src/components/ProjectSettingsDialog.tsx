import { Trash2 } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { openRemoveProjectDialog } from "../lib/projects";
import { invoke } from "../lib/tauri";
import { useStore } from "../store";
import { Button } from "./Button";

function close(): void {
  useStore.setState({ projectSettingsTarget: null });
}

function handleRemove(project: string): void {
  close();
  openRemoveProjectDialog(project);
}

function parseLines(value: string): string[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

export function ProjectSettingsDialog() {
  const project = useStore((s) => s.projectSettingsTarget);
  const [globsText, setGlobsText] = useState("");
  const [setupCommand, setSetupCommand] = useState("");
  const [runCommand, setRunCommand] = useState("");
  const [projectPrompt, setProjectPrompt] = useState("");
  const [envText, setEnvText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const firstInputRef = useRef<HTMLTextAreaElement | null>(null);
  const open = project !== null;
  const ref = useDialogRef(open);

  useEffect(() => {
    if (!project) return;
    setError(null);
    setLoading(true);
    invoke("get_project_settings", { project })
      .then((settings) => {
        setGlobsText(settings.copy_globs.join("\n"));
        setSetupCommand(settings.setup_command);
        setRunCommand(settings.run_command);
        setProjectPrompt(settings.project_prompt);
        setEnvText(settings.env.join("\n"));
      })
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, [project]);

  useEffect(() => {
    if (open) firstInputRef.current?.focus();
  }, [open]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!project) return;
    try {
      await invoke("save_project_settings", {
        project,
        settings: {
          copy_globs: parseLines(globsText),
          setup_command: setupCommand,
          run_command: runCommand,
          project_prompt: projectPrompt,
          env: parseLines(envText),
        },
      });
      close();
    } catch (err) {
      setError(String(err));
    }
  }

  if (!project)
    return <dialog id="project-settings-dialog" ref={ref} onClose={close} />;

  return (
    <dialog id="project-settings-dialog" ref={ref} onClose={close}>
      <form onSubmit={handleSubmit}>
        <h2>Provisioning for {project}</h2>
        {loading ? (
          <p className="hint">Loading…</p>
        ) : (
          <>
            <label>
              Files to copy into new worktrees (one glob/path per line)
              <textarea
                ref={firstInputRef}
                rows={4}
                value={globsText}
                placeholder={"e.g. CLAUDE.md\ndocs/*.md"}
                onChange={(e) => setGlobsText(e.target.value)}
              />
            </label>
            <label>
              Setup command (runs in the new worktree after files are copied —
              also a good place for `ln -s` commands if you'd rather symlink
              something than copy it)
              <textarea
                rows={6}
                value={setupCommand}
                placeholder="e.g. pnpm install && cargo fetch"
                onChange={(e) => setSetupCommand(e.target.value)}
              />
            </label>
            <label>
              Run command (used to prefill the Run tab — leave blank to
              auto-detect from package.json/Cargo.toml/pyproject.toml)
              <textarea
                rows={2}
                value={runCommand}
                placeholder="e.g. pnpm dev"
                onChange={(e) => setRunCommand(e.target.value)}
              />
            </label>
            <label>
              Agent intro prompt (project-specific context appended to every new
              session)
              <textarea
                rows={6}
                value={projectPrompt}
                placeholder="e.g. This repo's project-specific context for the agent"
                onChange={(e) => setProjectPrompt(e.target.value)}
              />
            </label>
            <label>
              Environment variables (one KEY=VALUE per line, injected into every
              agent session for this project)
              <textarea
                rows={4}
                value={envText}
                placeholder={"e.g. API_KEY=sk-...\nFEATURE_FLAGS=beta"}
                onChange={(e) => setEnvText(e.target.value)}
              />
            </label>
          </>
        )}
        {error && <p className="error">Error: {error}</p>}
        <menu>
          <Button
            variant="ghost-danger"
            className="mr-auto gap-1.5"
            onClick={() => handleRemove(project)}
          >
            <Trash2 size={14} /> Remove project
          </Button>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={loading}>
            Save
          </Button>
        </menu>
      </form>
    </dialog>
  );
}
