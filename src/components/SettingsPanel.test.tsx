import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import styles from "./SettingsForm.module.css";
import { SettingsPanel } from "./SettingsPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

/** Dispatch mocked invokes by command name. The settings form calls
 * `list_monospace_fonts` on mount, so order-based `mockResolvedValueOnce`
 * chains would hand a test's save-specific value to that call instead. */
function mockInvoke(
  handlers: Partial<Record<string, () => Promise<unknown>>>,
): void {
  vi.mocked(invoke).mockImplementation(((cmd: string) => {
    const handler = handlers[cmd];
    return handler ? handler() : Promise.resolve(undefined);
  }) as typeof invoke);
}

/** The backend's platform defaults as the form fetches them on mount. */
const PLATFORM_DEFAULTS = {
  editor_command: "zeditor",
  terminal_command: "ghostty",
};

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  // Answer the advisory mount-time fetch every test starts with. Tests that
  // care about the platform defaults override this via `mockInvoke`; leaving
  // it unmocked would make the form's `.then` throw on `undefined` in every
  // test that merely renders the panel.
  mockInvoke({
    platform_defaults: async () => PLATFORM_DEFAULTS,
  });
});

describe("SettingsPanel", () => {
  it("renders nothing interactive when closed", () => {
    render(<SettingsPanel />);
    expect(screen.queryByText("Settings")).not.toBeInTheDocument();
  });

  it("prefills from the current settings when opened", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: {
        ...s.settings,
        editor_command: "zed",
        terminal_font_size: 18,
      },
    }));
    render(<SettingsPanel />);
    expect(screen.getByDisplayValue("zed")).toBeInTheDocument();
    expect(screen.getByDisplayValue("18")).toBeInTheDocument();
  });

  it("lists the selectable harnesses without the retired OpenCode v1", () => {
    useStore.setState((s) => ({ settingsOpen: true, settings: s.settings }));
    render(<SettingsPanel />);
    const options = screen.getByLabelText("Harness").querySelectorAll("option");
    expect([...options].map((o) => o.textContent)).toEqual([
      "Claude Code",
      "OpenCode v2",
      "Pi",
    ]);
  });

  it("keeps the retired OpenCode v1 option while it's the current selection", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "opencode" },
    }));
    render(<SettingsPanel />);
    const options = screen.getByLabelText("Harness").querySelectorAll("option");
    expect([...options].map((o) => o.textContent)).toEqual([
      "Claude Code",
      "OpenCode v2",
      "OpenCode v1 (legacy)",
      "Pi",
    ]);
  });

  it("hides the Claude Code executable field while the opencode v1 harness is selected", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "opencode" },
    }));
    render(<SettingsPanel />);
    expect(
      screen.queryByText("Claude Code executable"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("OpenCode v1 executable")).toBeInTheDocument();
  });

  it("shows the Claude Code executable field while the claude harness is selected", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "claude" },
    }));
    render(<SettingsPanel />);
    expect(screen.getByText("Claude Code executable")).toBeInTheDocument();
    expect(
      screen.queryByText("OpenCode v1 executable"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("OpenCode v2 executable"),
    ).not.toBeInTheDocument();
  });

  it("shows the OpenCode v2 executable field while the opencode2 harness is selected", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "opencode2" },
    }));
    render(<SettingsPanel />);
    expect(screen.getByText("OpenCode v2 executable")).toBeInTheDocument();
    expect(
      screen.queryByText("Claude Code executable"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("OpenCode v1 executable"),
    ).not.toBeInTheDocument();
  });

  it("toggles the embedded browser setting only for the opencode2 harness", async () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "opencode2" },
    }));
    mockInvoke({ save_settings: async () => undefined });
    render(<SettingsPanel />);

    const toggle = screen.getByRole("checkbox", {
      name: /embedded browser/i,
    });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole("button", { name: /save/i }));

    await vi.waitFor(() =>
      expect(useStore.getState().settings.browser_enabled).toBe(false),
    );
  });

  it("hides the embedded browser setting under other harnesses", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "claude" },
    }));
    render(<SettingsPanel />);
    expect(
      screen.queryByRole("checkbox", { name: /embedded browser/i }),
    ).not.toBeInTheDocument();
  });

  it("shows the Pi executable field while the pi harness is selected", () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, harness: "pi" },
    }));
    render(<SettingsPanel />);
    expect(screen.getByText("Pi executable")).toBeInTheDocument();
    expect(
      screen.queryByText("Claude Code executable"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("OpenCode v2 executable"),
    ).not.toBeInTheDocument();
  });

  it("saves and applies the font size live, staying open after Save", async () => {
    const setFontSize = vi.fn();
    const setFontFamily = vi.fn();
    useStore.setState({
      settingsOpen: true,
      sessionMgr: { setFontSize, setFontFamily } as never,
    });
    mockInvoke({ save_settings: async () => undefined });
    render(<SettingsPanel />);
    fireEvent.change(
      screen.getByDisplayValue(
        String(useStore.getState().settings.terminal_font_size),
      ),
      {
        target: { value: "22" },
      },
    );
    fireEvent.click(screen.getByText("Save"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_settings", expect.anything()),
    );
    expect(setFontSize).toHaveBeenCalledWith(22);
    expect(useStore.getState().settingsOpen).toBe(true);
  });

  it("offers the system's installed monospaced fonts as suggestions", async () => {
    useStore.setState({ settingsOpen: true });
    mockInvoke({
      list_monospace_fonts: async () => [
        "CaskaydiaCove Nerd Font Mono",
        "JetBrains Mono",
      ],
    });
    render(<SettingsPanel />);

    const datalist = document.getElementById("terminal-font-family-list");
    expect(datalist).not.toBeNull();
    await vi.waitFor(() =>
      expect(
        [...(datalist?.querySelectorAll("option") ?? [])].map((o) => o.value),
      ).toEqual([
        "CaskaydiaCove Nerd Font Mono",
        "JetBrains Mono",
        // Always appended so the generic keyword is pickable even when the
        // backend enumeration returns a full list.
        "monospace",
      ]),
    );
  });

  it("warns when the configured font isn't installed on this machine", async () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, terminal_font_family: "Nonexistent Mono" },
    }));
    mockInvoke({
      list_monospace_fonts: async () => ["Hack", "JetBrains Mono"],
    });
    render(<SettingsPanel />);

    expect(
      await screen.findByText(/“Nonexistent Mono” isn't among/),
    ).toBeInTheDocument();
  });

  it("doesn't warn about an installed font", async () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, terminal_font_family: "Hack" },
    }));
    mockInvoke({ list_monospace_fonts: async () => ["Hack"] });
    render(<SettingsPanel />);

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_monospace_fonts", undefined),
    );
    expect(screen.queryByText(/isn't among/)).not.toBeInTheDocument();
  });

  it("doesn't claim a font is missing when the scan itself failed", async () => {
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, terminal_font_family: "Nonexistent Mono" },
    }));
    mockInvoke({
      list_monospace_fonts: async () => {
        throw new Error("not allowed");
      },
    });
    render(<SettingsPanel />);

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_monospace_fonts", undefined),
    );
    expect(screen.queryByText(/isn't among/)).not.toBeInTheDocument();
  });

  it("closes via the back button", () => {
    useStore.setState({ settingsOpen: true });
    render(<SettingsPanel />);
    fireEvent.click(screen.getByTitle("Back (Esc)"));
    expect(useStore.getState().settingsOpen).toBe(false);
  });

  it("has no bottom Close button", () => {
    useStore.setState({ settingsOpen: true });
    render(<SettingsPanel />);
    expect(screen.queryByText("Close")).not.toBeInTheDocument();
  });

  it("shows an error on save failure", async () => {
    useStore.setState({ settingsOpen: true });
    mockInvoke({
      save_settings: async () => {
        throw new Error("disk full");
      },
    });
    render(<SettingsPanel />);
    fireEvent.click(screen.getByText("Save"));
    await screen.findByText("Error: disk full");
  });

  it("persists the platform default when the command fields are blank", async () => {
    // Regression guard for the frontend/backend default split: the platform
    // default is macOS `zed`/`open`, so a blank field must save *that* — not
    // the hardcoded Linux `zeditor`/`ghostty`, which would silently pin a Mac
    // to a terminal command it doesn't have.
    mockInvoke({
      platform_defaults: async () => ({
        editor_command: "zed",
        terminal_command: "open",
      }),
      save_settings: async () => undefined,
    });
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: { ...s.settings, editor_command: "", terminal_command: "" },
    }));
    render(<SettingsPanel />);
    // Let the advisory mount fetch land so the placeholder reflects the
    // platform default — the save path reads it synchronously.
    await vi.waitFor(() =>
      expect(screen.getByPlaceholderText("zed")).toBeInTheDocument(),
    );

    // Submitted directly rather than by clicking: both command inputs are
    // `required`, so native constraint validation blocks a click-through on an
    // empty field. This is the state a settings.json with `""` loads into.
    fireEvent.submit(document.querySelector("form") as HTMLFormElement);

    await vi.waitFor(() => {
      const saved = useStore.getState().settings;
      expect(saved.editor_command).toBe("zed");
      expect(saved.terminal_command).toBe("open");
    });
  });

  it("never blanks the command fields when the platform defaults are unavailable", async () => {
    // The blank-field fallback must never produce an empty command just
    // because the advisory fetch failed — an empty `editor_command` would
    // make the "Zed" button spawn nothing.
    mockInvoke({
      platform_defaults: async () => {
        throw new Error("unavailable");
      },
      save_settings: async () => undefined,
    });
    useStore.setState((s) => ({
      settingsOpen: true,
      settings: {
        ...s.settings,
        editor_command: "",
        terminal_command: "",
      },
    }));
    render(<SettingsPanel />);
    fireEvent.submit(document.querySelector("form") as HTMLFormElement);

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_settings", expect.anything()),
    );
    const saved = useStore.getState().settings;
    expect(saved.editor_command).not.toBe("");
    expect(saved.terminal_command).not.toBe("");
  });

  it("shows a saving/saved sequence on the Save button, then reverts", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    useStore.setState({ settingsOpen: true });
    mockInvoke({ save_settings: async () => undefined });
    render(<SettingsPanel />);

    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await screen.findByText("Saved");

    vi.advanceTimersByTime(1500);
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: /save/i })).toHaveTextContent(
        "Save",
      ),
    );

    vi.useRealTimers();
  });

  it("marks the Save button with an error state on save failure", async () => {
    useStore.setState({ settingsOpen: true });
    mockInvoke({
      save_settings: async () => {
        throw new Error("disk full");
      },
    });
    render(<SettingsPanel />);

    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await screen.findByText("Error: disk full");

    expect(screen.getByRole("button", { name: /save/i })).toHaveClass(
      styles.saveBtnError,
    );
  });
});
