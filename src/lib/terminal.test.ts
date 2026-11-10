import type { Terminal } from "@xterm/xterm";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));
vi.mock("./toasts", () => ({ showError: vi.fn() }));

const { attachCopyOnCtrlC } = await import("./terminal");
const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");

type Handler = (e: KeyboardEvent) => boolean;

/** A terminal stub that captures the key handler `attachCopyOnCtrlC` installs,
 * so the test can fire synthetic keystrokes at it. Returns the raw stub (for
 * stubbing `hasSelection`/`getSelection`) plus a `fire` helper that runs the
 * captured handler. */
function makeTerm() {
  let handler: Handler | undefined;
  const stub = {
    attachCustomKeyEventHandler: vi.fn((h: Handler) => {
      handler = h;
    }),
    hasSelection: vi.fn(() => false),
    getSelection: vi.fn(() => ""),
  };
  return {
    term: stub as unknown as Terminal,
    stub,
    fire(e: KeyboardEvent): boolean {
      if (!handler) throw new Error("no key handler installed");
      return handler(e);
    },
  };
}

function keyEvent(partial: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    type: "keydown",
    key: "c",
    ctrlKey: true,
    altKey: false,
    metaKey: false,
    shiftKey: false,
    preventDefault: vi.fn(),
    ...partial,
  } as unknown as KeyboardEvent;
}

describe("attachCopyOnCtrlC", () => {
  beforeEach(() => {
    vi.mocked(writeText).mockReset();
    vi.mocked(writeText).mockResolvedValue(undefined);
  });

  it("copies the selection on Ctrl+C and suppresses the default", () => {
    const { term, stub, fire } = makeTerm();
    attachCopyOnCtrlC(term);
    stub.hasSelection.mockReturnValue(true);
    stub.getSelection.mockReturnValue("selected text");

    const event = keyEvent();
    expect(fire(event)).toBe(false);
    expect(writeText).toHaveBeenCalledWith("selected text");
    expect(event.preventDefault).toHaveBeenCalled();
  });

  it("copies on Ctrl+Shift+C too (the standard terminal chord)", () => {
    const { term, stub, fire } = makeTerm();
    attachCopyOnCtrlC(term);
    stub.hasSelection.mockReturnValue(true);
    stub.getSelection.mockReturnValue("selected text");

    expect(fire(keyEvent({ key: "C", shiftKey: true }))).toBe(false);
    expect(writeText).toHaveBeenCalledWith("selected text");
  });

  it("does nothing when there is no selection", () => {
    const { term, fire } = makeTerm();
    attachCopyOnCtrlC(term);

    const event = keyEvent();
    expect(fire(event)).toBe(true);
    expect(writeText).not.toHaveBeenCalled();
    expect(event.preventDefault).not.toHaveBeenCalled();
  });

  it("ignores non-ctrl and keyup events", () => {
    const { term, stub, fire } = makeTerm();
    attachCopyOnCtrlC(term);
    stub.hasSelection.mockReturnValue(true);

    expect(fire(keyEvent({ ctrlKey: false }))).toBe(true);
    expect(fire(keyEvent({ type: "keyup" }))).toBe(true);
    expect(writeText).not.toHaveBeenCalled();
  });
});
