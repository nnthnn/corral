import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { RunPanel } from "./RunPanel";
import styles from "./RunPanel.module.css";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

// jsdom has no real layout/canvas, so xterm.js can't render meaningfully in
// tests — mock it and assert against what was written rather than rendered
// DOM text.
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

describe("RunPanel", () => {
  it("prefills the command input from resolve_run_command on first view", async () => {
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "resolve_run_command"
        ? Promise.resolve("pnpm dev")
        : Promise.resolve(null),
    );
    render(<RunPanel wt={wt} />);

    await waitFor(() =>
      expect(screen.getByPlaceholderText("e.g. pnpm dev")).toHaveValue(
        "pnpm dev",
      ),
    );
  });

  it("starts a run, optimistically resetting output/running state", async () => {
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "resolve_run_command"
        ? Promise.resolve("pnpm dev")
        : cmd === "start_run"
          ? Promise.resolve(undefined)
          : Promise.resolve(null),
    );
    render(<RunPanel wt={wt} />);
    await waitFor(() =>
      expect(screen.getByPlaceholderText("e.g. pnpm dev")).toHaveValue(
        "pnpm dev",
      ),
    );

    fireEvent.click(screen.getByTitle("Run"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_run", {
        project: "corral",
        feature: "weeding",
        command: "pnpm dev",
      }),
    );
    expect(useStore.getState().run.get("corral/weeding")).toEqual({
      output: [],
      running: true,
      exit_code: null,
    });
  });

  it("shows an empty-state placeholder before anything has run, and hides it once there's run state", () => {
    const { rerender } = render(<RunPanel wt={wt} />);
    expect(screen.getByText(/Nothing running yet/)).toBeInTheDocument();

    useStore.setState({
      run: new Map([
        ["corral/weeding", { output: [104], running: true, exit_code: null }],
      ]),
    });
    rerender(<RunPanel wt={wt} />);
    expect(screen.queryByText(/Nothing running yet/)).not.toBeInTheDocument();
  });

  it("shows a Stop button only while running, and stops on click", async () => {
    useStore.setState({
      run: new Map([
        ["corral/weeding", { output: [104], running: true, exit_code: null }],
      ]),
    });
    vi.mocked(invoke).mockResolvedValue(undefined);
    render(<RunPanel wt={wt} />);

    expect(screen.getByTitle("Stop run")).toBeInTheDocument();
    expect(screen.queryByTitle("Run")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTitle("Stop run"));

    expect(invoke).toHaveBeenCalledWith("stop_run", { key: "corral/weeding" });
  });

  it("writes retained output into the terminal", async () => {
    useStore.setState({
      run: new Map([
        [
          "corral/weeding",
          { output: [104, 105], running: false, exit_code: 0 },
        ],
      ]),
    });
    vi.mocked(invoke).mockResolvedValue(undefined);
    render(<RunPanel wt={wt} />);

    await waitFor(() => expect(writtenText()).toBe("hi"));
  });

  it("shows the exit code once stopped", () => {
    useStore.setState({
      run: new Map([
        ["corral/weeding", { output: [], running: false, exit_code: 1 }],
      ]),
    });
    render(<RunPanel wt={wt} />);

    expect(screen.getByText("Exited (code 1)")).toBeInTheDocument();
  });

  it("shows an error toast when starting fails", async () => {
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "resolve_run_command"
        ? Promise.resolve("pnpm dev")
        : cmd === "start_run"
          ? Promise.reject(new Error("boom"))
          : Promise.resolve(null),
    );
    render(<RunPanel wt={wt} />);
    await waitFor(() =>
      expect(screen.getByPlaceholderText("e.g. pnpm dev")).toHaveValue(
        "pnpm dev",
      ),
    );

    fireEvent.click(screen.getByTitle("Run"));

    await waitFor(() =>
      expect(useStore.getState().toasts).toEqual([
        expect.objectContaining({
          message: "Run wouldn't start: Error: boom",
        }),
      ]),
    );
    expect(useStore.getState().run.get("corral/weeding")).toEqual({
      output: [],
      running: false,
      exit_code: null,
    });
  });

  it("lists justfile targets and runs one on click", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "resolve_run_command") return Promise.resolve("");
      if (cmd === "list_just_targets") {
        return Promise.resolve([
          { name: "dev", doc: "start the dev server", required_params: [] },
          { name: "test", doc: null, required_params: [] },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(<RunPanel wt={wt} />);

    const dev = await screen.findByRole("button", { name: "dev" });
    expect(screen.getByRole("button", { name: "test" })).toBeInTheDocument();
    expect(dev).toHaveAttribute("title", "just dev — start the dev server");
    expect(dev).not.toHaveClass(styles.selected);

    fireEvent.click(dev);

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("start_run", {
        project: "corral",
        feature: "weeding",
        command: "just dev",
      }),
    );
    expect(screen.getByPlaceholderText("e.g. pnpm dev")).toHaveValue(
      "just dev",
    );
    expect(dev).toHaveClass(styles.selected);
  });

  it("moves the highlight to the most recently clicked target", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "resolve_run_command") return Promise.resolve("");
      if (cmd === "list_just_targets") {
        return Promise.resolve([
          { name: "deploy", doc: null, required_params: ["env"] },
          { name: "release", doc: null, required_params: ["version"] },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(<RunPanel wt={wt} />);

    const deploy = await screen.findByRole("button", { name: "deploy" });
    const release = screen.getByRole("button", { name: "release" });

    fireEvent.click(deploy);
    expect(deploy).toHaveClass(styles.selected);
    expect(release).not.toHaveClass(styles.selected);

    fireEvent.click(release);
    expect(release).toHaveClass(styles.selected);
    expect(deploy).not.toHaveClass(styles.selected);
  });

  it("prefills instead of running a target that needs parameters", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "resolve_run_command") return Promise.resolve("");
      if (cmd === "list_just_targets") {
        return Promise.resolve([
          { name: "deploy", doc: null, required_params: ["env"] },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(<RunPanel wt={wt} />);

    const deploy = await screen.findByRole("button", { name: "deploy" });
    expect(deploy).toHaveAttribute("title", "just deploy — needs: env");

    fireEvent.click(deploy);

    expect(screen.getByPlaceholderText("e.g. pnpm dev")).toHaveValue(
      "just deploy ",
    );
    expect(invoke).not.toHaveBeenCalledWith("start_run", expect.anything());
  });

  it("disables justfile targets while a run is in progress", async () => {
    useStore.setState({
      run: new Map([
        ["corral/weeding", { output: [104], running: true, exit_code: null }],
      ]),
    });
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "list_just_targets"
        ? Promise.resolve([{ name: "dev", doc: null, required_params: [] }])
        : Promise.resolve(undefined),
    );
    render(<RunPanel wt={wt} />);

    expect(await screen.findByRole("button", { name: "dev" })).toBeDisabled();
  });
});

describe("RunPanel terminal refit", () => {
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

  it("resizes the run PTY when its container box changes", () => {
    useStore.setState({
      run: new Map([
        ["corral/weeding", { output: [104], running: true, exit_code: null }],
      ]),
    });
    const triggerResize = captureResizeObserver();
    render(<RunPanel wt={wt} />);
    const el = screen.getByTestId("run-terminal");
    Object.defineProperty(el, "clientWidth", {
      value: 640,
      configurable: true,
    });
    Object.defineProperty(el, "clientHeight", {
      value: 400,
      configurable: true,
    });
    vi.mocked(invoke).mockClear();

    triggerResize();

    expect(invoke).toHaveBeenCalledWith("resize_run", {
      key: "corral/weeding",
      rows: 24,
      cols: 80,
    });
  });

  it("does not resize the run PTY while the panel measures zero", () => {
    const triggerResize = captureResizeObserver();
    render(<RunPanel wt={wt} />);
    vi.mocked(invoke).mockClear();

    // clientWidth/clientHeight stay 0 (jsdom default) — the hidden-tab case.
    triggerResize();

    expect(invoke).not.toHaveBeenCalledWith("resize_run", expect.anything());
  });
});
