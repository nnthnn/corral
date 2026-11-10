import { listen } from "@tauri-apps/api/event";
import { useStore } from "../store";
import type { ProvisionState } from "../types";
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

/** Cap retained setup-command output, mirroring the PTY scrollback cap
 * (`scrollback_bytes` in `src-tauri/src/commands/session.rs`) — a chatty or
 * looping setup script shouldn't grow this array unbounded for the lifetime
 * of a long-running provisioning session. */
const MAX_PROVISION_OUTPUT_BYTES = 256 * 1024;

function appendOutput(
  prev: ProvisionState | undefined,
  data: number[],
): ProvisionState {
  const output = [...(prev?.output ?? []), ...data];
  const overflow = output.length - MAX_PROVISION_OUTPUT_BYTES;
  return {
    output: overflow > 0 ? output.slice(overflow) : output,
    running: true,
    exit_code: null,
  };
}

/** Backend streams a post-create setup command's output over these events
 * (see `src-tauri/src/commands/provision.rs`) — mirror them into the store
 * so `ProvisionPanel` can render live without polling. Called once from
 * `bootstrap.ts`. */
export async function initProvisioning(): Promise<void> {
  await listen<OutputEvent>("provision-output", (event) => {
    const { key, data } = event.payload;
    const provisioning = new Map(useStore.getState().provisioning);
    provisioning.set(
      key,
      appendOutput(provisioning.get(key), Array.from(decodeBase64(data))),
    );
    useStore.setState({ provisioning });
  });

  await listen<ExitEvent>("provision-exit", (event) => {
    const { key, code } = event.payload;
    const provisioning = new Map(useStore.getState().provisioning);
    const prev = provisioning.get(key);
    provisioning.set(key, {
      output: prev?.output ?? [],
      running: false,
      exit_code: code,
    });
    useStore.setState({ provisioning });
  });
}
