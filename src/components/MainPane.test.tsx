import { invoke } from "@tauri-apps/api/core";
import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import { MainPane } from "./MainPane";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../session", () => ({
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

describe("MainPane", () => {
  it("hides the agent pane while settings is open, even on the agent tab", () => {
    useStore.setState({ activeTab: "agent", settingsOpen: false });
    const { rerender } = render(<MainPane />);
    expect(document.getElementById("agent-pane")).not.toHaveAttribute("hidden");

    useStore.setState({ settingsOpen: true });
    rerender(<MainPane />);
    expect(document.getElementById("agent-pane")).toHaveAttribute("hidden");
  });

  it("adds a reselect-flash class while reselectFlash is set, then clears it", () => {
    vi.useFakeTimers();
    try {
      useStore.setState({ reselectFlash: true });
      render(<MainPane />);
      expect(document.querySelector(".main")).toHaveClass("reselect-flash");
      vi.advanceTimersByTime(150);
      expect(useStore.getState().reselectFlash).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not add the reselect-flash class by default", () => {
    render(<MainPane />);
    expect(document.querySelector(".main")).not.toHaveClass("reselect-flash");
  });
});
