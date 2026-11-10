import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, useStore } from "../store";
import { ResizeHandle } from "./ResizeHandle";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);
});

function renderSidebarHandle() {
  return render(
    <ResizeHandle
      settingsKey="sidebar_width"
      min={200}
      max={500}
      defaultWidth={320}
      side="right"
    />,
  );
}

describe("ResizeHandle", () => {
  it("increases sidebar_width when dragged right and clamps to max", () => {
    renderSidebarHandle();
    const handle = screen.getByTestId("resize-handle-sidebar_width");

    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 130 });
    expect(useStore.getState().settings.sidebar_width).toBe(350);

    fireEvent.pointerMove(window, { clientX: 1000 });
    expect(useStore.getState().settings.sidebar_width).toBe(500);

    fireEvent.pointerUp(window);
    expect(invoke).toHaveBeenCalledWith("save_settings", {
      settings: { ...DEFAULT_SETTINGS, sidebar_width: 500 },
    });
  });

  it("does not call save_settings while dragging, only on pointer up", () => {
    renderSidebarHandle();
    const handle = screen.getByTestId("resize-handle-sidebar_width");

    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 110 });
    fireEvent.pointerMove(window, { clientX: 120 });
    expect(invoke).not.toHaveBeenCalled();

    fireEvent.pointerUp(window);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("resets to defaultWidth and persists on double-click", () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, sidebar_width: 450 },
    }));
    renderSidebarHandle();
    const handle = screen.getByTestId("resize-handle-sidebar_width");

    fireEvent.doubleClick(handle);
    expect(useStore.getState().settings.sidebar_width).toBe(320);
    expect(invoke).toHaveBeenCalledWith("save_settings", {
      settings: { ...DEFAULT_SETTINGS, sidebar_width: 320 },
    });
  });

  it("flips the drag direction for a left-side handle", () => {
    render(
      <ResizeHandle
        settingsKey="changes_width"
        min={240}
        max={600}
        defaultWidth={384}
        side="left"
      />,
    );
    const handle = screen.getByTestId("resize-handle-changes_width");

    fireEvent.pointerDown(handle, { clientX: 100 });
    // Dragging left (negative delta) grows a left-edge handle's pane.
    fireEvent.pointerMove(window, { clientX: 70 });
    expect(useStore.getState().settings.changes_width).toBe(414);
  });

  it("increases sidebar_width by 10 on ArrowRight and persists immediately", () => {
    renderSidebarHandle();
    const handle = screen.getByTestId("resize-handle-sidebar_width");

    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(useStore.getState().settings.sidebar_width).toBe(330);
    expect(invoke).toHaveBeenCalledWith("save_settings", {
      settings: { ...DEFAULT_SETTINGS, sidebar_width: 330 },
    });
  });

  it("increases changes_width by 10 on ArrowLeft for a left-side handle", () => {
    render(
      <ResizeHandle
        settingsKey="changes_width"
        min={240}
        max={600}
        defaultWidth={384}
        side="left"
      />,
    );
    const handle = screen.getByTestId("resize-handle-changes_width");

    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(useStore.getState().settings.changes_width).toBe(394);
    expect(invoke).toHaveBeenCalledWith("save_settings", {
      settings: { ...DEFAULT_SETTINGS, changes_width: 394 },
    });
  });
});
