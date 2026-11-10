import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { ProvisionPanel } from "./ProvisionPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

// jsdom has no real layout/canvas, so xterm.js can't render meaningfully in
// tests — mock it and assert against what was written/reset rather than
// rendered DOM text.
const { writes, disposeMock, resetMock } = vi.hoisted(() => ({
  writes: [] as Uint8Array[],
  disposeMock: vi.fn(),
  resetMock: vi.fn(),
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;
    loadAddon() {}
    open() {}
    write(data: Uint8Array) {
      writes.push(data);
    }
    dispose() {
      disposeMock();
    }
    reset() {
      resetMock();
    }
    attachCustomKeyEventHandler() {}
    unicode = { activeVersion: "6" };
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));

const wt: Worktree = {
  project: "corral",
  feature: "weeding",
  slug: "weeding",
  branch: "feat/weeding",
  path: "/path",
  status: null,
  has_resumable_session: false,
  note: "",
  has_setup_command: true,
};

function writtenText(): string {
  return new TextDecoder().decode(
    new Uint8Array(writes.flatMap((chunk) => Array.from(chunk))),
  );
}

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);
  writes.length = 0;
  disposeMock.mockClear();
  resetMock.mockClear();
});

describe("ProvisionPanel", () => {
  it("explains when the project has no setup command configured", () => {
    render(<ProvisionPanel wt={{ ...wt, has_setup_command: false }} />);
    expect(screen.getByText("No setup command configured")).toBeInTheDocument();
    expect(
      screen.getByText(/No setup command configured for corral/),
    ).toBeInTheDocument();
    expect(screen.getByTitle("Re-run setup")).toBeDisabled();
  });

  it("shows a 'hasn't run' empty state with a usable Re-run before any output", () => {
    render(<ProvisionPanel wt={wt} />);
    expect(screen.getByText("Setup hasn't run")).toBeInTheDocument();
    expect(screen.getByText(/No setup output yet/)).toBeInTheDocument();
    expect(screen.getByTitle("Re-run setup")).toBeEnabled();
  });

  it("reattaches retained backend output on mount", async () => {
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "get_provision_output"
        ? Promise.resolve({ output: [104, 105], running: false, exit_code: 0 })
        : Promise.resolve(undefined),
    );
    render(<ProvisionPanel wt={wt} />);
    await waitFor(() =>
      expect(screen.getByText("Setup finished")).toBeInTheDocument(),
    );
    expect(writtenText()).toBe("hi");
  });

  it("shows running output, written into the terminal", () => {
    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: true, exit_code: null },
        ],
      ]),
    });
    render(<ProvisionPanel wt={wt} />);
    expect(screen.getByText("Running setup…")).toBeInTheDocument();
    expect(screen.getByTestId("provision-terminal")).toBeInTheDocument();
    expect(writtenText()).toBe("hi");
  });

  it("keeps the terminal visible once setup finishes successfully", () => {
    // The old docked panel collapsed to a one-line strip on success; the tab
    // now always shows the log, so the finished run stays readable.
    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: false, exit_code: 0 },
        ],
      ]),
    });
    render(<ProvisionPanel wt={wt} />);
    expect(screen.getByText("Setup finished")).toBeInTheDocument();
    expect(screen.getByTestId("provision-terminal")).toBeInTheDocument();
    expect(screen.queryByText(/No setup output yet/)).not.toBeInTheDocument();
  });

  it("labels a failed setup with its exit code", () => {
    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: false, exit_code: 1 },
        ],
      ]),
    });
    render(<ProvisionPanel wt={wt} />);
    expect(screen.getByText("Setup went down (exit 1)")).toBeInTheDocument();
    expect(screen.getByTestId("provision-terminal")).toBeInTheDocument();
  });

  it("replays the full backlog into a fresh terminal on mount", () => {
    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: false, exit_code: 0 },
        ],
      ]),
    });
    render(<ProvisionPanel wt={wt} />);
    expect(writtenText()).toBe("hi");
  });

  it("re-runs setup and optimistically resets output/running state", () => {
    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: false, exit_code: 1 },
        ],
      ]),
    });
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "rerun_provision"
        ? Promise.resolve(undefined)
        : Promise.resolve(null),
    );
    render(<ProvisionPanel wt={wt} />);

    fireEvent.click(screen.getByTitle("Re-run setup"));

    expect(invoke).toHaveBeenCalledWith("rerun_provision", {
      project: "corral",
      feature: "weeding",
    });
    expect(screen.getByText("Running setup…")).toBeInTheDocument();
    expect(
      useStore.getState().provisioning.get("corral/weeding")?.output,
    ).toEqual([]);
  });

  it("shows an error toast when re-run fails", async () => {
    useStore.setState({
      provisioning: new Map([
        ["corral/weeding", { output: [], running: false, exit_code: 1 }],
      ]),
    });
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "rerun_provision"
        ? Promise.reject(new Error("boom"))
        : Promise.resolve(null),
    );
    render(<ProvisionPanel wt={wt} />);

    fireEvent.click(screen.getByTitle("Re-run setup"));

    await waitFor(() =>
      expect(useStore.getState().toasts).toEqual([
        expect.objectContaining({
          message: "Setup wouldn't restart: Error: boom",
        }),
      ]),
    );
    expect(useStore.getState().provisioning.get("corral/weeding")).toEqual({
      output: [],
      running: false,
      exit_code: null,
    });
  });

  it("shows a Stop button only while setup is running", () => {
    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: false, exit_code: 0 },
        ],
      ]),
    });
    const { rerender } = render(<ProvisionPanel wt={wt} />);
    expect(screen.queryByTitle("Stop setup")).not.toBeInTheDocument();

    useStore.setState({
      provisioning: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: true, exit_code: null },
        ],
      ]),
    });
    rerender(<ProvisionPanel wt={wt} />);
    expect(screen.getByTitle("Stop setup")).toBeInTheDocument();
  });

  it("stops a running setup command", () => {
    useStore.setState({
      provisioning: new Map([
        ["corral/weeding", { output: [], running: true, exit_code: null }],
      ]),
    });
    vi.mocked(invoke).mockResolvedValue(undefined);
    render(<ProvisionPanel wt={wt} />);

    fireEvent.click(screen.getByTitle("Stop setup"));

    expect(invoke).toHaveBeenCalledWith("stop_provision", {
      key: "corral/weeding",
    });
  });

  it("shows an error toast when stopping fails", async () => {
    useStore.setState({
      provisioning: new Map([
        ["corral/weeding", { output: [], running: true, exit_code: null }],
      ]),
    });
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "stop_provision"
        ? Promise.reject(new Error("boom"))
        : Promise.resolve(null),
    );
    render(<ProvisionPanel wt={wt} />);

    fireEvent.click(screen.getByTitle("Stop setup"));

    await waitFor(() =>
      expect(useStore.getState().toasts).toEqual([
        expect.objectContaining({
          message: "Setup wouldn't stop: Error: boom",
        }),
      ]),
    );
  });
});

describe("ProvisionPanel terminal refit", () => {
  const originalResizeObserver = globalThis.ResizeObserver;

  /** Install a ResizeObserver stub that captures its callback, so a test can
   * simulate the container's box changing (jsdom never lays out). */
  function captureResizeObserver(): () => void {
    let trigger: (() => void) | undefined;
    globalThis.ResizeObserver = class {
      constructor(cb: () => void) {
        trigger = cb;
      }
      observe() {}
      disconnect() {}
      unobserve() {}
    } as unknown as typeof ResizeObserver;
    return () => trigger?.();
  }

  afterEach(() => {
    globalThis.ResizeObserver = originalResizeObserver;
  });

  it("resizes the setup PTY when its container box changes", () => {
    useStore.setState({
      provisioning: new Map([
        ["corral/weeding", { output: [104], running: true, exit_code: null }],
      ]),
    });
    const triggerResize = captureResizeObserver();
    render(<ProvisionPanel wt={wt} />);
    const el = screen.getByTestId("provision-terminal");
    Object.defineProperty(el, "clientWidth", {
      value: 640,
      configurable: true,
    });
    Object.defineProperty(el, "clientHeight", {
      value: 152,
      configurable: true,
    });
    vi.mocked(invoke).mockClear();

    triggerResize();

    expect(invoke).toHaveBeenCalledWith("resize_provision", {
      key: "corral/weeding",
      rows: 24,
      cols: 80,
    });
  });

  it("does not resize the setup PTY while the panel measures zero", () => {
    useStore.setState({
      provisioning: new Map([
        ["corral/weeding", { output: [104], running: true, exit_code: null }],
      ]),
    });
    const triggerResize = captureResizeObserver();
    render(<ProvisionPanel wt={wt} />);
    vi.mocked(invoke).mockClear();

    // clientWidth/clientHeight stay 0 (jsdom default).
    triggerResize();

    expect(invoke).not.toHaveBeenCalledWith(
      "resize_provision",
      expect.anything(),
    );
  });
});
