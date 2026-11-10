import { describe, expect, it } from "vitest";
import { classifyDiffLine, languageForPath, splitDiff } from "./diffView";

describe("classifyDiffLine", () => {
  it("classifies the diff header and meta lines", () => {
    expect(classifyDiffLine("diff --git a/x b/x")).toBe("meta");
    expect(classifyDiffLine("index abc..def 100644")).toBe("meta");
    expect(classifyDiffLine("--- a/src/main.rs")).toBe("meta");
    expect(classifyDiffLine("+++ b/src/main.rs")).toBe("meta");
    expect(classifyDiffLine("\\ No newline at end of file")).toBe("meta");
  });

  it("classifies hunk, add, del, and context lines", () => {
    expect(classifyDiffLine("@@ -1,3 +1,4 @@ fn main()")).toBe("hunk");
    expect(classifyDiffLine("+added")).toBe("add");
    expect(classifyDiffLine("-removed")).toBe("del");
    expect(classifyDiffLine(" context")).toBe("ctx");
  });

  it("treats a leading dash in a context line as context, not deletion", () => {
    // git prefixes context lines with a space, so "-foo" alone is a deletion;
    // " -foo" is a context line whose content starts with a dash.
    expect(classifyDiffLine("-foo")).toBe("del");
    expect(classifyDiffLine(" -foo")).toBe("ctx");
  });
});

describe("splitDiff", () => {
  it("splits and classifies each line", () => {
    expect(splitDiff("+a\n-b\n c")).toEqual([
      { text: "+a", kind: "add" },
      { text: "-b", kind: "del" },
      { text: " c", kind: "ctx" },
    ]);
  });
});

describe("languageForPath", () => {
  it("maps common extensions", () => {
    expect(languageForPath("src/main.ts")).toBe("typescript");
    expect(languageForPath("src/main.tsx")).toBe("typescript");
    expect(languageForPath("src/main.js")).toBe("javascript");
    expect(languageForPath("src/lib.rs")).toBe("rust");
    expect(languageForPath("script.py")).toBe("python");
    expect(languageForPath("data.json")).toBe("json");
    expect(languageForPath("style.css")).toBe("css");
    expect(languageForPath("style.scss")).toBe("scss");
    expect(languageForPath("index.html")).toBe("xml");
    expect(languageForPath("README.md")).toBe("markdown");
    expect(languageForPath("ci.yml")).toBe("yaml");
    expect(languageForPath("run.sh")).toBe("bash");
  });

  it("maps basename-only files", () => {
    expect(languageForPath("Dockerfile")).toBe("dockerfile");
    expect(languageForPath("api/Dockerfile")).toBe("dockerfile");
    expect(languageForPath("Makefile")).toBe("makefile");
  });

  it("returns null for unknown or extensionless files", () => {
    expect(languageForPath("LICENSE")).toBeNull();
    expect(languageForPath("src/unknown.zzz")).toBeNull();
    expect(languageForPath("trailing-dot.")).toBeNull();
  });
});
