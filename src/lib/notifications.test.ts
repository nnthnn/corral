import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import {
  initNotifications,
  maybeNotifyWaiting,
  notify,
  notifyExit,
  playSound,
  resetAudioContextForTests,
} from "./notifications";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
}));

const session = (overrides: Partial<SessionInfo> = {}): SessionInfo => ({
  id: "corral/hulling",
  project: "corral",
  feature: "hulling",
  running: true,
  exit_code: null,
  killed: false,
  waiting: true,
  last_summary: null,
  ...overrides,
});

const worktree = (overrides: Partial<Worktree> = {}): Worktree => ({
  project: "corral",
  feature: "hulling",
  slug: "corral/hulling",
  branch: "feat/hulling",
  path: "/tmp/hulling",
  status: null,
  has_resumable_session: false,
  note: "",
  ...overrides,
});

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(isPermissionGranted).mockReset();
  vi.mocked(requestPermission).mockReset();
});

describe("initNotifications", () => {
  it("requests permission best-effort and stores the notify icon", async () => {
    vi.mocked(isPermissionGranted).mockResolvedValue(false);
    vi.mocked(requestPermission).mockResolvedValue("granted");
    vi.mocked(invoke).mockResolvedValueOnce("/tmp/icon.png");
    await initNotifications();
    expect(requestPermission).toHaveBeenCalled();
    expect(useStore.getState().notifyIcon).toBe("/tmp/icon.png");
  });

  it("doesn't throw if permission APIs reject", async () => {
    vi.mocked(isPermissionGranted).mockRejectedValue(new Error("unsupported"));
    vi.mocked(invoke).mockRejectedValueOnce(new Error("no icon"));
    await expect(initNotifications()).resolves.toBeUndefined();
  });
});

describe("notify", () => {
  it("sends when notifications are enabled", () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, notifications_enabled: true },
    }));
    notify("title", "body");
    expect(invoke).toHaveBeenCalledWith("send_notification", {
      title: "title",
      body: "body",
      icon: null,
    });
  });

  it("does nothing when notifications are disabled", () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, notifications_enabled: false },
    }));
    notify("title", "body");
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("maybeNotifyWaiting", () => {
  it("notifies once per turn for a backgrounded session", () => {
    useStore.setState((s) => ({
      sessions: new Map([["corral/hulling", session()]]),
      settings: { ...s.settings, notifications_enabled: true },
      selectedId: null,
    }));
    maybeNotifyWaiting("corral/hulling");
    maybeNotifyWaiting("corral/hulling");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("doesn't notify for the worktree currently selected and focused", () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    useStore.setState((s) => ({
      sessions: new Map([["corral/hulling", session()]]),
      settings: { ...s.settings, notifications_enabled: true },
      selectedId: "corral/hulling",
    }));
    maybeNotifyWaiting("corral/hulling");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("uses the worktree's renamed title in the body, not the raw feature slug", () => {
    useStore.setState((s) => ({
      sessions: new Map([["corral/hulling", session()]]),
      worktrees: [worktree({ title: "Fix Login Timeout" })],
      settings: { ...s.settings, notifications_enabled: true },
      selectedId: null,
    }));
    maybeNotifyWaiting("corral/hulling");
    expect(invoke).toHaveBeenCalledWith(
      "send_notification",
      expect.objectContaining({ body: "Fix Login Timeout" }),
    );
  });

  it("falls back to the raw feature slug when the worktree isn't in the store", () => {
    useStore.setState((s) => ({
      sessions: new Map([["corral/hulling", session()]]),
      worktrees: [],
      settings: { ...s.settings, notifications_enabled: true },
      selectedId: null,
    }));
    maybeNotifyWaiting("corral/hulling");
    expect(invoke).toHaveBeenCalledWith(
      "send_notification",
      expect.objectContaining({ body: "hulling" }),
    );
  });
});

describe("notifyExit", () => {
  it("uses the worktree's renamed title in the body", () => {
    useStore.setState((s) => ({
      worktrees: [worktree({ title: "Fix Login Timeout" })],
      settings: { ...s.settings, notifications_enabled: true },
      selectedId: null,
    }));
    notifyExit("corral/hulling", "corral", "hulling", 0, false);
    expect(invoke).toHaveBeenCalledWith(
      "send_notification",
      expect.objectContaining({ body: "Fix Login Timeout" }),
    );
  });
});

describe("playSound", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    // The module now caches a single AudioContext across calls; reset it so
    // each test's stubbed AudioContext constructor is picked up fresh
    // instead of a previous test's cached instance being reused.
    resetAudioContextForTests();
  });

  it("does nothing when sound is disabled", () => {
    const AudioContextMock = vi.fn();
    vi.stubGlobal("AudioContext", AudioContextMock);
    useStore.setState((s) => ({
      settings: { ...s.settings, sound_enabled: false },
    }));
    playSound();
    expect(AudioContextMock).not.toHaveBeenCalled();
  });

  it("plays two tones when sound is enabled", () => {
    const start = vi.fn();
    const oscillator = {
      frequency: { value: 0 },
      connect: vi.fn(),
      start,
      stop: vi.fn(),
    };
    const gain = {
      connect: vi.fn(),
      gain: {
        setValueAtTime: vi.fn(),
        exponentialRampToValueAtTime: vi.fn(),
      },
    };
    class AudioContextMock {
      currentTime = 0;
      createOscillator = vi.fn(() => oscillator);
      createGain = vi.fn(() => gain);
      destination = {};
    }
    vi.stubGlobal("AudioContext", AudioContextMock as unknown);
    useStore.setState((s) => ({
      settings: { ...s.settings, sound_enabled: true },
    }));
    playSound();
    expect(start).toHaveBeenCalledTimes(2);
  });

  it("doesn't throw if AudioContext is unavailable", () => {
    vi.stubGlobal("AudioContext", undefined);
    useStore.setState((s) => ({
      settings: { ...s.settings, sound_enabled: true },
    }));
    expect(() => playSound()).not.toThrow();
  });
});
