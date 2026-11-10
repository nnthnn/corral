import { fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "./store";

const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(() => ({ onDragDropEvent: vi.fn() })),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readImage: vi.fn(),
  readText: vi.fn(),
  writeText: vi.fn(),
}));

const attachSession = vi.fn();
const writeSession = vi.fn().mockResolvedValue(undefined);
const resizeSession = vi.fn().mockResolvedValue(undefined);
vi.mock("./lib/tauri", () => ({
  invoke: (cmd: string, ...args: unknown[]) => {
    if (cmd === "attach_session") return attachSession(...args);
    if (cmd === "write_session") return writeSession(...args);
    if (cmd === "resize_session") return resizeSession(...args);
    return Promise.resolve();
  },
}));

class FakeTerminal {
  static instances: FakeTerminal[] = [];
  constructor() {
    FakeTerminal.instances.push(this);
  }
  write = vi.fn();
  loadAddon = vi.fn();
  open = vi.fn();
  keyHandler: ((e: KeyboardEvent) => boolean) | undefined;
  attachCustomKeyEventHandler = vi.fn(
    (handler: (e: KeyboardEvent) => boolean) => {
      this.keyHandler = handler;
    },
  );
  onData = vi.fn();
  dispose = vi.fn();
  focus = vi.fn();
  hasSelection = vi.fn(() => false);
  getSelection = vi.fn(() => "");
  paste = vi.fn();
  options: Record<string, unknown> = {};
  rows = 24;
  cols = 80;
  unicode = { activeVersion: "6" };
}
vi.mock("@xterm/xterm", () => ({ Terminal: FakeTerminal }));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  },
}));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    onContextLoss = vi.fn();
    dispose = vi.fn();
  },
}));

const { SessionManager } = await import("./session");
const { readImage, readText, writeText } = await import(
  "@tauri-apps/plugin-clipboard-manager"
);

function keyEvent(key: string): KeyboardEvent {
  return {
    type: "keydown",
    key,
    ctrlKey: true,
    altKey: false,
    metaKey: false,
    preventDefault: vi.fn(),
  } as unknown as KeyboardEvent;
}

function fire(name: string, payload: unknown) {
  listeners.get(name)?.({ payload });
}

describe("SessionManager restart overlay", () => {
  let container: HTMLDivElement;

  beforeEach(() => {
    listeners.clear();
    attachSession.mockReset();
    attachSession.mockResolvedValue({ info: {}, buffer: [] });
    resizeSession.mockClear();
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  it("shows a Restart button on a non-deliberate exit and calls onRestart when clicked", async () => {
    const onRestart = vi.fn();
    const mgr = new SessionManager({ container, fontSize: 13, onRestart });
    await mgr.show("corral/hulling");

    fire("session-exit", { id: "corral/hulling", code: 1, killed: false });

    const btn = container.querySelector("button.restart-overlay");
    expect(btn).not.toBeNull();

    fireEvent.click(btn as HTMLButtonElement);
    expect(onRestart).toHaveBeenCalledWith("corral/hulling");
    expect(container.querySelector("button.restart-overlay")).toBeNull();
  });

  it("does not show a Restart button on a deliberate stop", async () => {
    const onRestart = vi.fn();
    const mgr = new SessionManager({ container, fontSize: 13, onRestart });
    await mgr.show("corral/hulling");

    fire("session-exit", { id: "corral/hulling", code: null, killed: true });

    expect(container.querySelector("button.restart-overlay")).toBeNull();
    expect(onRestart).not.toHaveBeenCalled();
  });

  it("uses a harness-neutral terminal exit message", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    const term = FakeTerminal.instances[FakeTerminal.instances.length - 1];

    fire("session-exit", { id: "corral/hulling", code: 130, killed: false });

    expect(term?.write).toHaveBeenCalledWith(
      "\r\n\x1b[90m[agent exited (130)]\x1b[0m\r\n",
    );
  });

  it("stops the cursor blinking once the session ends, restoring it on new output", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    const term = FakeTerminal.instances[FakeTerminal.instances.length - 1];
    term.options.cursorBlink = true;

    fire("session-exit", { id: "corral/hulling", code: 0, killed: false });
    expect(term.options.cursorBlink).toBe(false);
    expect(term.options.cursorInactiveStyle).toBe("none");
    // The runtime DECSCUSR override the agent's TUI set must be cleared too,
    // or xterm keeps blinking regardless of `options.cursorBlink`.
    expect(term.write).toHaveBeenCalledWith("\x1b[0 q");

    fire("session-output", { id: "corral/hulling", data: "YQ==" });
    expect(term.options.cursorBlink).toBe(true);
    expect(term.options.cursorInactiveStyle).toBe("outline");
  });

  it("clears a stale Restart button once new output arrives", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");

    fire("session-exit", { id: "corral/hulling", code: 0, killed: false });
    expect(container.querySelector("button.restart-overlay")).not.toBeNull();

    fire("session-output", { id: "corral/hulling", data: "YQ==" });
    expect(container.querySelector("button.restart-overlay")).toBeNull();
  });

  it("replaces a stale Restart button on a second non-deliberate exit", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");

    fire("session-exit", { id: "corral/hulling", code: 1, killed: false });
    const first = container.querySelector("button.restart-overlay");
    expect(first).not.toBeNull();

    fire("session-exit", { id: "corral/hulling", code: 1, killed: false });
    expect(container.querySelectorAll("button.restart-overlay")).toHaveLength(
      1,
    );
  });

  it("auto-falls back to a fresh session instead of showing Restart when resume failed", async () => {
    const onRestart = vi.fn();
    const onResumeFailed = vi.fn();
    const mgr = new SessionManager({
      container,
      fontSize: 13,
      onRestart,
      onResumeFailed,
    });
    await mgr.show("corral/hulling");

    fire("session-exit", {
      id: "corral/hulling",
      code: 1,
      killed: false,
      resume_failed: true,
    });

    expect(container.querySelector("button.restart-overlay")).toBeNull();
    expect(onResumeFailed).toHaveBeenCalledWith("corral/hulling");
    expect(onRestart).not.toHaveBeenCalled();
  });

  it("calls onExit before onResumeFailed, so a restart callback never reads stale running state", async () => {
    const order: string[] = [];
    const mgr = new SessionManager({
      container,
      fontSize: 13,
      onExit: () => order.push("onExit"),
      onResumeFailed: () => order.push("onResumeFailed"),
    });
    await mgr.show("corral/hulling");

    fire("session-exit", {
      id: "corral/hulling",
      code: 1,
      killed: false,
      resume_failed: true,
    });

    expect(order).toEqual(["onExit", "onResumeFailed"]);
  });
});

describe("SessionManager resize reannounce", () => {
  let container: HTMLDivElement;

  beforeEach(() => {
    listeners.clear();
    attachSession.mockReset();
    attachSession.mockResolvedValue({ info: {}, buffer: [] });
    resizeSession.mockClear();
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  it("re-sends the terminal size once real output arrives for the visible session", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    resizeSession.mockClear();

    fire("session-output", { id: "corral/hulling", data: "YQ==" });

    expect(resizeSession).toHaveBeenCalledWith({
      id: "corral/hulling",
      rows: 24,
      cols: 80,
    });
  });

  it("only re-sends the size once per session, even across multiple output events", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    resizeSession.mockClear();

    fire("session-output", { id: "corral/hulling", data: "YQ==" });
    fire("session-output", { id: "corral/hulling", data: "Yg==" });

    expect(resizeSession).toHaveBeenCalledTimes(1);
  });

  it("re-sends the size again after resetSizeCache, for a fresh PTY on the same worktree", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    fire("session-output", { id: "corral/hulling", data: "YQ==" });
    expect(resizeSession).toHaveBeenCalledTimes(1);

    // A second start_session for the same worktree spawns a brand-new PTY
    // (back to the hardcoded default) — without resetting the cache, the
    // dedup in fitActive/reannounce would wrongly assume nothing changed.
    resizeSession.mockClear();
    mgr.resetSizeCache("corral/hulling");
    fire("session-output", { id: "corral/hulling", data: "Yg==" });

    expect(resizeSession).toHaveBeenCalledWith({
      id: "corral/hulling",
      rows: 24,
      cols: 80,
    });
  });

  it("resetSizeCache is a no-op for an id with no terminal entry", () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    expect(() => mgr.resetSizeCache("corral/no-such-session")).not.toThrow();
  });

  it("does not re-send the size for a backgrounded session's output", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    await mgr.show("corral/milking");
    resizeSession.mockClear();

    fire("session-output", { id: "corral/hulling", data: "YQ==" });

    expect(resizeSession).not.toHaveBeenCalled();
  });
});

describe("SessionManager refit on container resize", () => {
  let container: HTMLDivElement;
  const originalResizeObserver = globalThis.ResizeObserver;

  /** Install a ResizeObserver stub that captures the callback so a test can
   * simulate the container's box changing (jsdom never lays out, so the real
   * observer never fires). */
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

  beforeEach(() => {
    listeners.clear();
    attachSession.mockReset();
    attachSession.mockResolvedValue({ info: {}, buffer: [] });
    resizeSession.mockClear();
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  afterEach(() => {
    globalThis.ResizeObserver = originalResizeObserver;
  });

  it("refits the visible terminal when the container's box changes", async () => {
    const triggerResize = captureResizeObserver();
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    resizeSession.mockClear();

    // jsdom reports 0×0, so fake a real box (e.g. the sidebar just collapsed).
    Object.defineProperty(container, "clientWidth", {
      value: 800,
      configurable: true,
    });
    Object.defineProperty(container, "clientHeight", {
      value: 600,
      configurable: true,
    });
    triggerResize();

    expect(resizeSession).toHaveBeenCalledWith({
      id: "corral/hulling",
      rows: 24,
      cols: 80,
    });
  });

  it("does not shrink the PTY while the pane is hidden", async () => {
    const triggerResize = captureResizeObserver();
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    resizeSession.mockClear();

    // clientWidth/clientHeight remain 0 — the display:none case.
    triggerResize();

    expect(resizeSession).not.toHaveBeenCalled();
  });
});

describe("SessionManager copy/paste key handling", () => {
  let container: HTMLDivElement;

  beforeEach(() => {
    listeners.clear();
    attachSession.mockReset();
    attachSession.mockResolvedValue({ info: {}, buffer: [] });
    FakeTerminal.instances.length = 0;
    vi.mocked(readText).mockReset();
    vi.mocked(readImage).mockReset();
    vi.mocked(writeText).mockReset();
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  it("prevents the native paste event so xterm's own textarea listener can't double-insert", async () => {
    vi.mocked(readText).mockResolvedValue("pasted text");
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    const term = FakeTerminal.instances[0];

    const event = keyEvent("v");
    term.keyHandler?.(event);
    await vi.waitFor(() =>
      expect(term.paste).toHaveBeenCalledWith("pasted text"),
    );

    expect(event.preventDefault).toHaveBeenCalled();
    expect(term.paste).toHaveBeenCalledTimes(1);
  });

  it("falls back to reading a clipboard image when readText rejects (image-only clipboard)", async () => {
    vi.mocked(readText).mockRejectedValue(new Error("no text on clipboard"));
    vi.mocked(readImage).mockRejectedValue(new Error("no image on clipboard"));
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    const term = FakeTerminal.instances[0];

    term.keyHandler?.(keyEvent("v"));
    await vi.waitFor(() => expect(readImage).toHaveBeenCalled());

    expect(term.paste).not.toHaveBeenCalled();
  });

  it("prevents the native copy event when a selection is copied", async () => {
    vi.mocked(writeText).mockResolvedValue(undefined);
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    const term = FakeTerminal.instances[0];
    term.hasSelection.mockReturnValue(true);
    term.getSelection.mockReturnValue("selected text");

    const event = keyEvent("c");
    term.keyHandler?.(event);
    await vi.waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("selected text"),
    );

    expect(event.preventDefault).toHaveBeenCalled();
  });
});

describe("SessionManager prompt submission", () => {
  let container: HTMLDivElement;

  beforeEach(() => {
    listeners.clear();
    attachSession.mockReset();
    attachSession.mockResolvedValue({ info: {}, buffer: [] });
    writeSession.mockReset();
    writeSession.mockResolvedValue(undefined);
    FakeTerminal.instances.length = 0;
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  it("pastes text into the active session's terminal and submits it", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    const term = FakeTerminal.instances[0];

    mgr.pasteToActive("hello from a check");

    expect(term.paste).toHaveBeenCalledWith("hello from a check");
    expect(writeSession).toHaveBeenCalledWith({
      id: "corral/hulling",
      data: "\r",
    });
  });

  it("does nothing when there's no active session", () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    expect(mgr.pasteToActive("no active session")).toBe(false);
    expect(writeSession).not.toHaveBeenCalled();
  });

  it("submits reliably to a specific session, echoing into that terminal only", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    await mgr.show("corral/milking");
    const hulling = FakeTerminal.instances[0];
    const milking = FakeTerminal.instances[1];

    expect(await mgr.submitTo("corral/hulling", "prepare the PR")).toBe(true);

    // Echo the injected prompt into the intended terminal (so it's visible
    // in raw mode), not the currently-active `milking` terminal.
    expect(hulling.paste).toHaveBeenCalledWith("prepare the PR");
    expect(milking.paste).not.toHaveBeenCalled();
    // The paste echoes + sends the text; only the Enter keystroke is written
    // directly (and awaited, so a dead session still reports failure).
    expect(writeSession).not.toHaveBeenCalledWith({
      id: "corral/hulling",
      data: "prepare the PR",
    });
    expect(writeSession).toHaveBeenCalledWith({
      id: "corral/hulling",
      data: "\r",
    });
  });

  it("reports a backend submission failure", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    writeSession.mockRejectedValueOnce(new Error("session exited"));

    expect(await mgr.submitTo("corral/hulling", "prepare the PR")).toBe(false);
  });
});

describe("SessionManager opencode2 update hint", () => {
  let container: HTMLDivElement;

  beforeEach(() => {
    listeners.clear();
    attachSession.mockReset();
    attachSession.mockResolvedValue({ info: {}, buffer: [] });
    useStore.setState({ toasts: [] });
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  afterEach(() => {
    useStore.setState({
      settings: {
        ...useStore.getState().settings,
        harness: "claude",
      },
    });
  });

  const KNOWN_ERROR_B64 =
    "SW52YWxpZCBvcGVuY29kZS9vcGVuYWktY29tcGF0aWJsZS1jaGF0IHN0cmVhbSBldmVudA==";

  it("surfaces an update hint once when an opencode2 session emits a known error", async () => {
    useStore.setState({
      settings: { ...useStore.getState().settings, harness: "opencode2" },
    });
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");

    fire("session-output", {
      id: "corral/hulling",
      data: KNOWN_ERROR_B64,
    });

    expect(
      useStore
        .getState()
        .toasts.some((t) => t.message.includes("updating OpenCode v2")),
    ).toBe(true);
  });

  it("does not hint for a non-opencode2 harness", async () => {
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");

    fire("session-output", {
      id: "corral/hulling",
      data: KNOWN_ERROR_B64,
    });

    expect(useStore.getState().toasts).toHaveLength(0);
  });

  it("re-arms the hint after a worktree id is disposed and re-created", async () => {
    useStore.setState({
      settings: { ...useStore.getState().settings, harness: "opencode2" },
    });
    const mgr = new SessionManager({ container, fontSize: 13 });
    await mgr.show("corral/hulling");
    fire("session-output", { id: "corral/hulling", data: KNOWN_ERROR_B64 });
    const hintToasts = () =>
      useStore
        .getState()
        .toasts.filter((t) => t.message.includes("updating OpenCode v2"));
    expect(hintToasts()).toHaveLength(1);

    // Removing and re-creating the same <project>/<feature> reuses the id, so
    // dispose must drop the "already hinted" mark or the hint never fires again.
    mgr.closeTerminal("corral/hulling");
    await mgr.show("corral/hulling");
    fire("session-output", { id: "corral/hulling", data: KNOWN_ERROR_B64 });

    expect(hintToasts()).toHaveLength(2);
  });
});
