import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import { initRun } from "./run-events";

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

describe("initRun", () => {
  it("appends output chunks for a key across multiple events", async () => {
    await initRun();
    listeners.get("run-output")?.({
      payload: { key: "corral/weeding", data: "aGk=" },
    });
    listeners.get("run-output")?.({
      payload: { key: "corral/weeding", data: "IQ==" },
    });

    const state = useStore.getState().run.get("corral/weeding");
    expect(state?.output).toEqual([104, 105, 33]);
    expect(state?.running).toBe(true);
    expect(state?.exit_code).toBeNull();
  });

  it("marks a run finished on run-exit, keeping its output", async () => {
    await initRun();
    listeners.get("run-output")?.({
      payload: { key: "corral/weeding", data: "aA==" },
    });
    listeners.get("run-exit")?.({
      payload: { key: "corral/weeding", code: 0 },
    });

    const state = useStore.getState().run.get("corral/weeding");
    expect(state?.output).toEqual([104]);
    expect(state?.running).toBe(false);
    expect(state?.exit_code).toBe(0);
  });
});
