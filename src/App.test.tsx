import { invoke } from "@tauri-apps/api/core";
import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { useStore } from "./store";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("0.1.0"),
}));
vi.mock("./session", () => ({
  SessionManager: vi.fn().mockImplementation(function SessionManager() {
    return {
      show: vi.fn(),
      hideAll: vi.fn(),
      has: vi.fn().mockReturnValue(false),
    };
  }),
  THEME: {
    background: "#1a1510",
    foreground: "#ede8e0",
    cursor: "#c49a3a",
    selectionBackground: "rgba(237, 232, 224, 0.16)",
  },
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;
    loadAddon() {}
    open() {}
    write() {}
    dispose() {}
    reset() {}
    attachCustomKeyEventHandler() {}
    unicode = { activeVersion: "6" };
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);
});

describe("App", () => {
  it("renders the sidebar, main pane, and changes pane together", () => {
    useStore.setState({
      worktrees: [
        {
          project: "corral",
          feature: "hulling",
          slug: "hulling",
          branch: "feat/hulling",
          path: "/path",
          status: null,
          has_resumable_session: false,
          note: "",
        },
      ],
      selectedId: "corral/hulling",
    });
    render(<App />);
    expect(
      screen.getByPlaceholderText("Filter worktrees…"),
    ).toBeInTheDocument(); // sidebar header
    expect(screen.getByText("No agent in this pen.")).toBeInTheDocument(); // main pane
    expect(screen.getByText("Changes")).toBeInTheDocument(); // changes pane
  });

  it("toggles body classes for collapsed panes and open settings", () => {
    render(<App />);
    expect(document.body.classList.contains("sidebar-collapsed")).toBe(false);

    // Store updates made outside a user event (fireEvent) aren't
    // automatically wrapped in act() by React Testing Library, so the
    // effect that syncs the body class wouldn't have flushed yet by the
    // time of a bare synchronous assertion.
    act(() => {
      useStore.setState((s) => ({
        settings: { ...s.settings, sidebar_collapsed: true },
      }));
    });
    expect(document.body.classList.contains("sidebar-collapsed")).toBe(true);

    act(() => {
      useStore.setState({ settingsOpen: true });
    });
    expect(document.body.classList.contains("settings-full")).toBe(true);
  });

  it("sets grid-column CSS variables from the stored pane widths", () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, sidebar_width: 400, changes_width: 300 },
    }));
    render(<App />);
    const app = document.querySelector(".app") as HTMLElement;
    expect(app.style.getPropertyValue("--sidebar-width")).toBe("400px");
    expect(app.style.getPropertyValue("--changes-width")).toBe("300px");
  });
});
