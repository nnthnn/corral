import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrap } from "./bootstrap";
import { needsCustomChrome } from "./lib/window-chrome";
import { useStore } from "./store";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
}));
vi.mock("./lib/window-chrome", () => ({ needsCustomChrome: vi.fn() }));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset().mockResolvedValue(vi.fn());
  vi.mocked(isPermissionGranted).mockReset().mockResolvedValue(true);
  vi.mocked(requestPermission).mockReset();
  vi.mocked(needsCustomChrome).mockReset().mockResolvedValue(false);
});

describe("bootstrap", () => {
  it("loads settings, initializes notifications, and loads worktrees", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        editor_command: "zeditor",
        notifications_enabled: true,
        terminal_font_size: 13,
        collapsed_groups: [],
        sidebar_collapsed: false,
        changes_collapsed: false,
        extra_instructions: "",
      }) // get_settings
      .mockResolvedValueOnce("/home/alice") // home_dir
      .mockResolvedValueOnce("/tmp/icon.png") // notify_icon
      .mockResolvedValueOnce([]) // list_worktrees
      .mockResolvedValueOnce([]); // list_sessions
    await bootstrap();
    expect(useStore.getState().notifyIcon).toBe("/tmp/icon.png");
    expect(useStore.getState().homeDir).toBe("/home/alice");
    expect(listen).toHaveBeenCalledWith("fs-changed", expect.any(Function));
  });

  it("sets needsCustomChrome from the window-chrome check", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        editor_command: "zeditor",
        notifications_enabled: true,
        terminal_font_size: 13,
        collapsed_groups: [],
        sidebar_collapsed: false,
        changes_collapsed: false,
        extra_instructions: "",
      }) // get_settings
      .mockResolvedValueOnce("/home/alice") // home_dir
      .mockResolvedValueOnce("/tmp/icon.png") // notify_icon
      .mockResolvedValueOnce([]) // list_worktrees
      .mockResolvedValueOnce([]); // list_sessions
    vi.mocked(needsCustomChrome).mockResolvedValue(true);
    await bootstrap();
    expect(useStore.getState().needsCustomChrome).toBe(true);
  });
});
