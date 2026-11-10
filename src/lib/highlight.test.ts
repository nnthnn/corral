import { describe, expect, it } from "vitest";
import { highlightCode } from "./highlight";

describe("highlightCode", () => {
  it("wraps keywords in token spans for a known language", () => {
    const html = highlightCode("fn main()", "rust");
    expect(html).toContain("hljs-keyword");
    expect(html).toContain("fn");
  });

  it("escapes input rather than passing through raw HTML", () => {
    const html = highlightCode("<script>alert(1)</script>", "rust");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });

  it("falls back to escaped plain text for unknown languages", () => {
    expect(highlightCode("a < b", "not-a-language")).toBe("a &lt; b");
  });

  it("treats null as plain text", () => {
    expect(highlightCode("a < b", null)).toBe("a &lt; b");
  });
});
