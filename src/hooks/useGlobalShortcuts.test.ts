import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, useStore } from "../store";
import type { Worktree } from "../types";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

const wt = (feature: string, project = "corral"): Worktree => ({
  project,
  feature,
  slug: feature,
  branch: `feat/${feature}`,
  path: `/path/${feature}`,
  status: null,
  has_resumable_session: false,
  note: "",
});

function keydown(key: string, opts: Partial<KeyboardEventInit> = {}) {
  const event = new KeyboardEvent("keydown", {
    key,
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
    ...opts,
  });
  window.dispatchEvent(event);
  return event;
}

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
});

describe("useGlobalShortcuts", () => {
  it("Ctrl+N selects the Nth worktree", () => {
    useStore.setState({
      projects: ["corral"],
      worktrees: [wt("hulling"), wt("churning")],
    });
    renderHook(() => useGlobalShortcuts());
    keydown("2");
    expect(useStore.getState().selectedId).toBe("corral/churning");
  });

  it("Ctrl+N follows the sidebar's custom project order", () => {
    useStore.setState({
      projects: ["corral", "other"],
      worktrees: [wt("hulling"), wt("planting", "other")],
      settings: { ...DEFAULT_SETTINGS, project_order: ["other", "corral"] },
    });
    renderHook(() => useGlobalShortcuts());
    keydown("1");
    expect(useStore.getState().selectedId).toBe("other/planting");
  });

  it("Ctrl+N skips worktrees hidden by the sidebar filter", () => {
    useStore.setState({
      projects: ["corral"],
      worktrees: [wt("hulling"), wt("churning")],
      filterText: "churn",
    });
    renderHook(() => useGlobalShortcuts());

    const outOfRange = keydown("2");
    expect(outOfRange.defaultPrevented).toBe(false);
    expect(useStore.getState().selectedId).toBeNull();

    keydown("1");
    expect(useStore.getState().selectedId).toBe("corral/churning");
  });

  it("Ctrl+] steps to the next worktree, wrapping past the end", () => {
    useStore.setState({
      projects: ["corral"],
      worktrees: [wt("hulling"), wt("churning"), wt("harrowing")],
      selectedId: "corral/harrowing",
    });
    renderHook(() => useGlobalShortcuts());
    keydown("]");
    expect(useStore.getState().selectedId).toBe("corral/hulling");
  });

  it("Ctrl+[ steps to the prev worktree, wrapping before the start", () => {
    useStore.setState({
      projects: ["corral"],
      worktrees: [wt("hulling"), wt("churning"), wt("harrowing")],
      selectedId: "corral/hulling",
    });
    renderHook(() => useGlobalShortcuts());
    keydown("[");
    expect(useStore.getState().selectedId).toBe("corral/harrowing");
  });

  it("Ctrl+] with nothing selected yet selects the first worktree", () => {
    useStore.setState({
      projects: ["corral"],
      worktrees: [wt("hulling"), wt("churning")],
      selectedId: null,
    });
    renderHook(() => useGlobalShortcuts());
    keydown("]");
    expect(useStore.getState().selectedId).toBe("corral/hulling");
  });

  it("Ctrl+, opens settings", () => {
    renderHook(() => useGlobalShortcuts());
    keydown(",");
    expect(useStore.getState().settingsOpen).toBe(true);
  });

  it("Ctrl+K toggles the command palette", () => {
    renderHook(() => useGlobalShortcuts());
    expect(useStore.getState().paletteOpen).toBe(false);

    keydown("k");
    expect(useStore.getState().paletteOpen).toBe(true);

    keydown("k");
    expect(useStore.getState().paletteOpen).toBe(false);
  });

  it("Ctrl++/-/0 delegate to sessionMgr's font-size methods", () => {
    const increaseFontSize = vi.fn();
    const decreaseFontSize = vi.fn();
    const resetFontSize = vi.fn();
    useStore.setState({
      sessionMgr: {
        increaseFontSize,
        decreaseFontSize,
        resetFontSize,
      } as never,
    });
    renderHook(() => useGlobalShortcuts());
    keydown("+");
    keydown("-");
    keydown("0");
    expect(increaseFontSize).toHaveBeenCalled();
    expect(decreaseFontSize).toHaveBeenCalled();
    expect(resetFontSize).toHaveBeenCalled();
  });

  it("Ctrl++/-/0 zoom diff_font_size instead of the terminal when the diff tab is active", () => {
    const increaseFontSize = vi.fn();
    useStore.setState({
      activeTab: "diff",
      settings: { ...DEFAULT_SETTINGS, diff_font_size: 16 },
      sessionMgr: { increaseFontSize } as never,
    });
    renderHook(() => useGlobalShortcuts());

    keydown("+");
    expect(useStore.getState().settings.diff_font_size).toBe(17);
    expect(increaseFontSize).not.toHaveBeenCalled();

    keydown("-");
    expect(useStore.getState().settings.diff_font_size).toBe(16);

    useStore.setState((s) => ({
      settings: { ...s.settings, diff_font_size: 30 },
    }));
    keydown("0");
    expect(useStore.getState().settings.diff_font_size).toBe(16);
  });

  it("diff_font_size zoom clamps to 8-32", () => {
    useStore.setState({
      activeTab: "diff",
      settings: { ...DEFAULT_SETTINGS, diff_font_size: 32 },
    });
    renderHook(() => useGlobalShortcuts());
    keydown("+");
    expect(useStore.getState().settings.diff_font_size).toBe(32);

    useStore.setState((s) => ({
      settings: { ...s.settings, diff_font_size: 8 },
    }));
    keydown("-");
    expect(useStore.getState().settings.diff_font_size).toBe(8);
  });

  it("Escape closes settings first, then the active non-agent tab", () => {
    useStore.setState({
      settingsOpen: true,
      activeTab: "diff",
      diffTab: { path: "x" },
    });
    renderHook(() => useGlobalShortcuts());
    // Dispatched on `document`, not `window` — handleEscape listens on
    // `document` in the bubble phase, and an event dispatched directly on
    // `window` never reaches a `document` listener (nothing bubbles "up" to
    // the very top of the hierarchy from itself).
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(useStore.getState().settingsOpen).toBe(false);
    expect(useStore.getState().activeTab).toBe("diff"); // first Escape only closes settings

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(useStore.getState().activeTab).toBe("agent");
    expect(useStore.getState().diffTab).toBeNull();
  });

  it("Escape does not close a tab while a modal dialog is open", () => {
    useStore.setState({
      deleteTarget: wt("hulling"),
      activeTab: "diff",
      diffTab: { path: "x" },
    });
    renderHook(() => useGlobalShortcuts());
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(useStore.getState().activeTab).toBe("diff");
    expect(useStore.getState().diffTab).toEqual({ path: "x" });
  });

  it("Escape closes the palette before settings", () => {
    useStore.setState({ paletteOpen: true, settingsOpen: true });
    renderHook(() => useGlobalShortcuts());

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(useStore.getState().paletteOpen).toBe(false);
    expect(useStore.getState().settingsOpen).toBe(true); // untouched this pass
  });

  describe("Ctrl+Tab tab cycling", () => {
    it("steps through the visible tabs and wraps at the end", () => {
      useStore.setState({
        worktrees: [wt("hulling")],
        selectedId: "corral/hulling",
        prInfoByWtId: new Map([
          [
            "corral/hulling",
            { number: 7, url: "u", state: "open", is_draft: false },
          ],
        ]),
        diffTab: { path: "x" },
        activeTab: "agent",
      });
      renderHook(() => useGlobalShortcuts());

      const event = keydown("Tab");
      expect(event.defaultPrevented).toBe(true);
      expect(useStore.getState().activeTab).toBe("run");
      keydown("Tab");
      expect(useStore.getState().activeTab).toBe("setup");
      keydown("Tab");
      expect(useStore.getState().activeTab).toBe("pr");
      keydown("Tab");
      expect(useStore.getState().activeTab).toBe("diff");
      keydown("Tab");
      expect(useStore.getState().activeTab).toBe("agent");
    });

    it("Ctrl+Shift+Tab steps backward, wrapping before the start", () => {
      useStore.setState({
        worktrees: [wt("hulling")],
        selectedId: "corral/hulling",
        diffTab: { path: "x" },
        activeTab: "agent",
      });
      renderHook(() => useGlobalShortcuts());

      keydown("Tab", { shiftKey: true });
      expect(useStore.getState().activeTab).toBe("diff");
    });

    it("skips tabs that aren't present, wrapping to the first visible tab", () => {
      // No PR/diff/file, so the visible set is just [agent, run, setup];
      // stepping past the last wraps back to the first.
      useStore.setState({
        worktrees: [wt("hulling")],
        selectedId: "corral/hulling",
        activeTab: "setup",
      });
      renderHook(() => useGlobalShortcuts());

      keydown("Tab");
      expect(useStore.getState().activeTab).toBe("agent");
    });

    it("does nothing when no worktree is selected", () => {
      renderHook(() => useGlobalShortcuts());
      const event = keydown("Tab");
      expect(event.defaultPrevented).toBe(false);
      expect(useStore.getState().activeTab).toBe("agent");
    });

    it("leaves the tabs alone while a full-window overlay is open", () => {
      useStore.setState({
        worktrees: [wt("hulling")],
        selectedId: "corral/hulling",
        activeTab: "agent",
        settingsOpen: true,
      });
      renderHook(() => useGlobalShortcuts());
      const event = keydown("Tab");
      expect(event.defaultPrevented).toBe(false);
      expect(useStore.getState().activeTab).toBe("agent");
    });
  });
});
