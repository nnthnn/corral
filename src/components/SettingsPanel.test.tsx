import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import styles from "./SettingsForm.module.css";
import { SettingsPanel } from "./SettingsPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
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
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
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
    vi.mocked(invoke).mockRejectedValueOnce(new Error("disk full"));
    render(<SettingsPanel />);
    fireEvent.click(screen.getByText("Save"));
    await screen.findByText("Error: disk full");
  });

  it("shows a saving/saved sequence on the Save button, then reverts", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    useStore.setState({ settingsOpen: true });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
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
    vi.mocked(invoke).mockRejectedValueOnce(new Error("disk full"));
    render(<SettingsPanel />);

    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await screen.findByText("Error: disk full");

    expect(screen.getByRole("button", { name: /save/i })).toHaveClass(
      styles.saveBtnError,
    );
  });
});
