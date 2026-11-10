import { listen } from "@tauri-apps/api/event";
import { useStore } from "../store";
import type { RunState } from "../types";
import { decodeBase64 } from "./base64";

interface OutputEvent {
  key: string;
  /** Base64-encoded PTY bytes — see `base64_bytes` in the backend. */
  data: string;
}

interface ExitEvent {
  key: string;
  code: number | null;
}

function appendOutput(prev: RunState | undefined, data: number[]): RunState {
  return {
    output: [...(prev?.output ?? []), ...data],
    running: true,
    exit_code: null,
  };
}

/** Backend streams the Run tab's dev-server output over these events (see
 * `src-tauri/src/commands/run.rs`) — mirror them into the store so
 * `RunPanel` can render live without polling. Called once from
 * `bootstrap.ts`. */
export async function initRun(): Promise<void> {
  await listen<OutputEvent>("run-output", (event) => {
    const { key, data } = event.payload;
    const run = new Map(useStore.getState().run);
    run.set(key, appendOutput(run.get(key), Array.from(decodeBase64(data))));
    useStore.setState({ run });
  });

  await listen<ExitEvent>("run-exit", (event) => {
    const { key, code } = event.payload;
    const run = new Map(useStore.getState().run);
    const prev = run.get(key);
    run.set(key, {
      output: prev?.output ?? [],
      running: false,
      exit_code: code,
    });
    useStore.setState({ run });
  });
}
