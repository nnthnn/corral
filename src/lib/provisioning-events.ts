import { listen } from "@tauri-apps/api/event";
import { useStore } from "../store";
import type { ProvisionState } from "../types";
import { decodeBase64 } from "./base64";
import {
  appendBoundedOutput,
  MAX_PROVISION_OUTPUT_BYTES,
} from "./output-buffer";

interface OutputEvent {
  key: string;
  /** Base64-encoded PTY bytes — see `base64_bytes` in the backend. */
  data: string;
}

interface ExitEvent {
  key: string;
  code: number | null;
}

function appendOutput(
  prev: ProvisionState | undefined,
  data: number[],
): ProvisionState {
  return {
    ...appendBoundedOutput(prev, data, MAX_PROVISION_OUTPUT_BYTES),
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
      dropped: prev?.dropped ?? 0,
      running: false,
      exit_code: code,
    });
    useStore.setState({ provisioning });
  });
}
