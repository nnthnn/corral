import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, useStore } from "../store";
import { loadSettings, saveSettings, scheduleSettingsSave } from "./settings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.useFakeTimers();
});

describe("loadSettings", () => {
  it("populates settings and collapsedGroups from the backend", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ...DEFAULT_SETTINGS,
      collapsed_groups: ["corral"],
    });
    await loadSettings();
    expect(useStore.getState().settings.collapsed_groups).toEqual(["corral"]);
    expect(useStore.getState().collapsedGroups).toEqual(new Set(["corral"]));
  });

  it("falls back to defaults when the backend call fails", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("nope"));
    await loadSettings();
    expect(useStore.getState().settings).toEqual(DEFAULT_SETTINGS);
  });
});

describe("saveSettings", () => {
  it("invokes save_settings with the current settings", async () => {
    useStore.setState({
      settings: { ...DEFAULT_SETTINGS, terminal_font_size: 20 },
    });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await saveSettings();
    expect(invoke).toHaveBeenCalledWith("save_settings", {
      settings: { ...DEFAULT_SETTINGS, terminal_font_size: 20 },
    });
  });
});

describe("scheduleSettingsSave", () => {
  it("debounces to a single save_settings call", () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    scheduleSettingsSave();
    scheduleSettingsSave();
    scheduleSettingsSave();
    vi.advanceTimersByTime(600);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
