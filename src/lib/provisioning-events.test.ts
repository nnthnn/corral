import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import { initProvisioning } from "./provisioning-events";

const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  listeners.clear();
});

describe("initProvisioning", () => {
  it("appends output chunks for a key across multiple events", async () => {
    await initProvisioning();
    listeners.get("provision-output")?.({
      payload: { key: "corral/weeding", data: "aGk=" },
    });
    listeners.get("provision-output")?.({
      payload: { key: "corral/weeding", data: "IQ==" },
    });

    const state = useStore.getState().provisioning.get("corral/weeding");
    expect(state?.output).toEqual([104, 105, 33]);
    expect(state?.running).toBe(true);
    expect(state?.exit_code).toBeNull();
  });

  it("marks a run finished on provision-exit, keeping its output", async () => {
    await initProvisioning();
    listeners.get("provision-output")?.({
      payload: { key: "corral/weeding", data: "aA==" },
    });
    listeners.get("provision-exit")?.({
      payload: { key: "corral/weeding", code: 0 },
    });

    const state = useStore.getState().provisioning.get("corral/weeding");
    expect(state?.output).toEqual([104]);
    expect(state?.running).toBe(false);
    expect(state?.exit_code).toBe(0);
  });

  it("caps retained output instead of growing unbounded", async () => {
    await initProvisioning();
    const cap = 256 * 1024;
    listeners.get("provision-output")?.({
      // 256KB of 0x01 bytes, base64-encoded.
      payload: { key: "corral/weeding", data: btoa("\x01".repeat(cap)) },
    });
    listeners.get("provision-output")?.({
      payload: { key: "corral/weeding", data: "AgM=" },
    });

    const state = useStore.getState().provisioning.get("corral/weeding");
    expect(state?.output.length).toBe(cap);
    // The oldest bytes were trimmed, so the tail is the most recent chunk.
    expect(state?.output.slice(-2)).toEqual([2, 3]);
  });
});
