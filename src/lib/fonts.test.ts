import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));
vi.mock("./toasts", () => ({ showError: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const { invoke } = await import("@tauri-apps/api/core");
const { FALLBACK_MONOSPACE_FONTS, firstUninstalledFamily, monospaceFonts } =
  await import("./fonts");

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

describe("monospaceFonts", () => {
  it("returns the scanned families with the generic keyword appended", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(["Hack", "JetBrains Mono"]);
    await expect(monospaceFonts()).resolves.toEqual({
      families: ["Hack", "JetBrains Mono", "monospace"],
      enumerated: true,
    });
  });

  it("de-duplicates families the backend already reported", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(["Hack", "monospace"]);
    const fonts = await monospaceFonts();
    expect(fonts.families).toEqual(["Hack", "monospace"]);
  });

  it("falls back to the hardcoded suggestions on failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("not allowed"));
    await expect(monospaceFonts()).resolves.toEqual({
      families: FALLBACK_MONOSPACE_FONTS,
      enumerated: false,
    });
  });

  it("falls back when the backend found nothing", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await expect(monospaceFonts()).resolves.toEqual({
      families: FALLBACK_MONOSPACE_FONTS,
      enumerated: false,
    });
  });
});

describe("firstUninstalledFamily", () => {
  const installed = ["Hack", "JetBrains Mono"];

  it("returns nothing for a blank stack or one made of generics", () => {
    expect(firstUninstalledFamily("", installed)).toBeNull();
    expect(firstUninstalledFamily("   ", installed)).toBeNull();
    expect(firstUninstalledFamily("monospace", installed)).toBeNull();
    expect(firstUninstalledFamily("ui-monospace", installed)).toBeNull();
    expect(firstUninstalledFamily("sans-serif", installed)).toBeNull();
  });

  it("returns nothing when a concrete family in the stack is installed", () => {
    expect(firstUninstalledFamily("Hack", installed)).toBeNull();
    expect(firstUninstalledFamily("hack, monospace", installed)).toBeNull();
    // One hit anywhere in the stack is enough — the others are only fallbacks.
    expect(firstUninstalledFamily("Fira Code, Hack", installed)).toBeNull();
  });

  it("returns the first missing family when none of them are installed", () => {
    expect(firstUninstalledFamily("Fira Code", installed)).toBe("Fira Code");
    expect(firstUninstalledFamily('"Fira Code", "Iosevka"', installed)).toBe(
      "Fira Code",
    );
  });
});
