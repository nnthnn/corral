import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CTRL_HOLD_HINT_DELAY_MS, useCtrlHoldHint } from "./useCtrlHoldHint";

function down(key: string) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key }));
}

function up(key: string) {
  window.dispatchEvent(new KeyboardEvent("keyup", { key }));
}

function reveal(result: { current: boolean }) {
  act(() => down("Control"));
  act(() => vi.advanceTimersByTime(CTRL_HOLD_HINT_DELAY_MS));
  expect(result.current).toBe(true);
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useCtrlHoldHint", () => {
  it("reveals once Ctrl has been held for the delay", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    act(() => down("Control"));

    act(() => vi.advanceTimersByTime(CTRL_HOLD_HINT_DELAY_MS - 1));
    expect(result.current).toBe(false);

    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
  });

  it("hides as soon as Ctrl is released", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    reveal(result);

    act(() => up("Control"));
    expect(result.current).toBe(false);
  });

  it("never reveals for a short tap", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    act(() => down("Control"));
    act(() => vi.advanceTimersByTime(1000));
    act(() => up("Control"));
    act(() => vi.advanceTimersByTime(CTRL_HOLD_HINT_DELAY_MS * 2));
    expect(result.current).toBe(false);
  });

  it("treats Meta like Ctrl", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    act(() => down("Meta"));
    act(() => vi.advanceTimersByTime(CTRL_HOLD_HINT_DELAY_MS));
    expect(result.current).toBe(true);

    act(() => up("Meta"));
    expect(result.current).toBe(false);
  });

  it("stays revealed while another modifier is still down", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    reveal(result);

    act(() => down("Meta"));
    act(() => up("Control"));
    expect(result.current).toBe(true);

    act(() => up("Meta"));
    expect(result.current).toBe(false);
  });

  it("does not restart the timer on key auto-repeat", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    act(() => down("Control"));
    act(() => vi.advanceTimersByTime(1500));

    // The OS repeats keydown while the key is held; the original timer must
    // keep running from the first press.
    act(() => down("Control"));
    act(() => vi.advanceTimersByTime(500));
    expect(result.current).toBe(true);
  });

  it("resets when the window loses focus with Ctrl still down", () => {
    const { result } = renderHook(() => useCtrlHoldHint());
    reveal(result);

    // No keyup arrives when Alt-Tab steals focus.
    act(() => window.dispatchEvent(new Event("blur")));
    expect(result.current).toBe(false);
  });
});
