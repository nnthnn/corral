import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import { dismissToast, showError, showSuccess } from "./toasts";

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.useFakeTimers();
});

describe("showError", () => {
  it("pushes an error toast with no variant", () => {
    showError("boom");
    const toasts = useStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ message: "boom" });
    expect(toasts[0].variant).toBeUndefined();
  });

  it("auto-dismisses after the default delay", () => {
    showError("boom");
    vi.advanceTimersByTime(5000);
    expect(useStore.getState().toasts).toHaveLength(0);
  });

  it("stays past the default delay when sticky, but dismisses after 15s", () => {
    showError("boom", { sticky: true });
    vi.advanceTimersByTime(5000);
    expect(useStore.getState().toasts).toHaveLength(1);
    vi.advanceTimersByTime(10_000);
    expect(useStore.getState().toasts).toHaveLength(0);
  });
});

describe("showSuccess", () => {
  it("pushes a success-variant toast", () => {
    showSuccess("yay");
    const toasts = useStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ message: "yay", variant: "success" });
  });

  it("auto-dismisses after the default delay", () => {
    showSuccess("yay");
    vi.advanceTimersByTime(5000);
    expect(useStore.getState().toasts).toHaveLength(0);
  });
});

describe("dismissToast", () => {
  it("removes only the toast with the matching id", () => {
    showError("first", { sticky: true });
    showError("second", { sticky: true });
    const [first, second] = useStore.getState().toasts;
    dismissToast(first.id);
    const remaining = useStore.getState().toasts;
    expect(remaining).toHaveLength(1);
    expect(remaining[0].id).toBe(second.id);
  });
});
