import { describe, expect, it } from "vitest";
import { detectOpencode2Hint } from "./opencode2-hint";

describe("detectOpencode2Hint", () => {
  it("detects the known invalid-stream-event error", () => {
    const hint = detectOpencode2Hint(
      "Invalid opencode/openai-compatible-chat stream event",
    );
    expect(hint).toContain("updating OpenCode v2");
  });

  it("detects the error through ANSI color codes", () => {
    const hint = detectOpencode2Hint(
      "\x1b[31mInvalid opencode/openai-compatible-chat stream event\x1b[0m",
    );
    expect(hint).toContain("updating OpenCode v2");
  });

  it("returns null for unrelated output", () => {
    expect(detectOpencode2Hint("compiling...")).toBeNull();
    expect(detectOpencode2Hint("")).toBeNull();
  });
});
