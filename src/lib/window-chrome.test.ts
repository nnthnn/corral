import { getCurrentWindow } from "@tauri-apps/api/window";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { closeWindow, needsCustomChrome } from "./window-chrome";

const mockWindow = {
  isDecorated: vi.fn(),
  close: vi.fn(),
};

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(getCurrentWindow).mockReturnValue(
    // biome-ignore lint/suspicious/noExplicitAny: partial mock of the real Window type
    mockWindow as any,
  );
  for (const fn of Object.values(mockWindow)) fn.mockReset();
});

describe("needsCustomChrome", () => {
  it("resolves false when the compositor granted real decorations", async () => {
    mockWindow.isDecorated.mockResolvedValue(true);
    expect(await needsCustomChrome()).toBe(false);
  });

  it("resolves true when the compositor did not grant decorations", async () => {
    mockWindow.isDecorated.mockResolvedValue(false);
    expect(await needsCustomChrome()).toBe(true);
  });

  it("resolves true (fail toward showing controls) when the check itself throws", async () => {
    mockWindow.isDecorated.mockRejectedValue(new Error("no permission"));
    expect(await needsCustomChrome()).toBe(true);
  });
});

describe("closeWindow", () => {
  it("calls Window.close", async () => {
    await closeWindow();
    expect(mockWindow.close).toHaveBeenCalled();
  });
});
