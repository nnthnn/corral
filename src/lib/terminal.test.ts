import type { Terminal } from "@xterm/xterm";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));
vi.mock("./toasts", () => ({ showError: vi.fn() }));

const {
  applyTerminalFont,
  attachCopyOnCtrlC,
  DEFAULT_TERMINAL_FONT,
  resolveTerminalFontFamily,
} = await import("./terminal");
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

describe("resolveTerminalFontFamily", () => {
  it("uses the built-in monospace stack when the setting is blank", () => {
    expect(resolveTerminalFontFamily("")).toBe(DEFAULT_TERMINAL_FONT);
    expect(resolveTerminalFontFamily("   ")).toBe(DEFAULT_TERMINAL_FONT);
  });

  it("appends the built-in stack as a fallback to a single configured family", () => {
    // Without a fallback, an uninstalled family (e.g. the datalist's "Fira
    // Code" on a machine that doesn't have it) makes WebKit fall back to its
    // default proportional font, which breaks xterm's cell grid. Using the
    // full built-in stack rather than a bare `monospace` makes a missing
    // custom font degrade exactly like the blank setting (e.g. to SF Mono on
    // macOS).
    expect(resolveTerminalFontFamily("Fira Code")).toBe(
      `Fira Code, ${DEFAULT_TERMINAL_FONT}`,
    );
    expect(resolveTerminalFontFamily('  "JetBrains Mono"  ')).toBe(
      `"JetBrains Mono", ${DEFAULT_TERMINAL_FONT}`,
    );
  });

  it("leaves a stack that already ends in a generic family alone", () => {
    expect(resolveTerminalFontFamily("Hack, monospace")).toBe(
      "Hack, monospace",
    );
    expect(resolveTerminalFontFamily("ui-monospace")).toBe("ui-monospace");
    expect(resolveTerminalFontFamily('"Fira Code", monospace')).toBe(
      '"Fira Code", monospace',
    );
  });
});

describe("applyTerminalFont", () => {
  function fakeTerm(
    options: { fontFamily?: string; fontSize?: number } = {},
  ): Terminal {
    return {
      options: { fontFamily: "old", fontSize: 13, ...options },
    } as never;
  }

  it("sets the resolved stack and size, reporting a change", () => {
    const term = fakeTerm();
    expect(applyTerminalFont(term, "Fira Code", 18)).toBe(true);
    expect(term.options.fontFamily).toBe(`Fira Code, ${DEFAULT_TERMINAL_FONT}`);
    expect(term.options.fontSize).toBe(18);
  });

  it("no-ops when the terminal already has those options", () => {
    // This is the mount case: the terminal is created with the current
    // settings, so the settings-subscription effect must not refit/resize.
    const term = fakeTerm({
      fontFamily: DEFAULT_TERMINAL_FONT,
      fontSize: 13,
    });
    expect(applyTerminalFont(term, "", 13)).toBe(false);
    expect(applyTerminalFont(term, "   ", 13)).toBe(false);
  });

  it("reports a change when only the size differs", () => {
    const term = fakeTerm({ fontFamily: DEFAULT_TERMINAL_FONT });
    expect(applyTerminalFont(term, "", 16)).toBe(true);
    expect(term.options.fontSize).toBe(16);
  });
});
