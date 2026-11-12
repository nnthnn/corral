import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Worktree } from "../types";
import {
  isResolvableImageSrc,
  renderMarkdown,
  resolveMarkdownImages,
  resolveRelativePath,
} from "./markdown";
import { invoke } from "./tauri";

vi.mock("./tauri", () => ({ invoke: vi.fn() }));

const wt: Worktree = {
  project: "corral",
  feature: "hulling",
  slug: "hulling",
  branch: "feat/hulling",
  path: "/path",
  status: null,
  has_resumable_session: false,
  note: "",
};

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

describe("renderMarkdown", () => {
  it("renders common GFM constructs", () => {
    const html = renderMarkdown("# Title\n\n- one\n- two\n\n**bold**");
    expect(html).toContain("<h1");
    expect(html).toContain("Title");
    expect(html).toContain("<li>one</li>");
    expect(html).toContain("<strong>bold</strong>");
  });

  it("syntax-highlights fenced code blocks", () => {
    const html = renderMarkdown("```rust\nfn main() {}\n```");
    expect(html).toContain('class="language-rust"');
    expect(html).toContain("hljs-keyword");
    expect(html).toContain("fn");
  });

  it("sanitizes scripts and event handlers out of raw HTML", () => {
    const html = renderMarkdown(
      '<script>alert(1)</script>\n\n<img src="x" onerror="alert(2)">',
    );
    expect(html).not.toContain("<script");
    expect(html).not.toContain("onerror");
    expect(html).not.toContain("alert");
  });
});

describe("isResolvableImageSrc", () => {
  it("accepts relative paths", () => {
    expect(isResolvableImageSrc("img/x.png")).toBe(true);
    expect(isResolvableImageSrc("../assets/x.png")).toBe(true);
    expect(isResolvableImageSrc("/x.png")).toBe(true);
  });

  it("rejects external, data, protocol-relative, and fragment srcs", () => {
    for (const src of [
      "https://example.com/x.png",
      "http://example.com/x.png",
      "data:image/png;base64,AAA",
      "//example.com/x.png",
      "#anchor",
      "",
    ]) {
      expect(isResolvableImageSrc(src)).toBe(false);
    }
  });
});

describe("resolveRelativePath", () => {
  it("resolves against the markdown file's own directory", () => {
    expect(resolveRelativePath("docs/guide", "img/x.png")).toBe(
      "docs/guide/img/x.png",
    );
    expect(resolveRelativePath("", "x.png")).toBe("x.png");
  });

  it("treats a leading slash as worktree-root-relative", () => {
    expect(resolveRelativePath("docs", "/assets/x.png")).toBe("assets/x.png");
  });

  it("collapses . and .. segments", () => {
    expect(resolveRelativePath("docs", "./a/../b/x.png")).toBe("docs/b/x.png");
    expect(resolveRelativePath("docs/guide", "../x.png")).toBe("docs/x.png");
  });

  it("returns null when the path escapes the worktree", () => {
    expect(resolveRelativePath("", "../../etc/passwd")).toBeNull();
    expect(resolveRelativePath("docs", "../../etc/passwd")).toBeNull();
  });

  it("strips query/fragment and percent-decodes", () => {
    expect(resolveRelativePath("", "x.png?raw=1#frag")).toBe("x.png");
    expect(resolveRelativePath("", "my%20image.png")).toBe("my image.png");
  });
});

describe("resolveMarkdownImages", () => {
  it("inlines a relative image read from the worktree", async () => {
    vi.mocked(invoke).mockResolvedValue({
      kind: "image",
      mime: "image/png",
      data: "AAA",
    });
    const html = await resolveMarkdownImages(
      wt,
      "docs/readme.md",
      '<p><img src="img/x.png" alt="x"></p>',
    );
    expect(invoke).toHaveBeenCalledWith("read_worktree_file", {
      project: "corral",
      feature: "hulling",
      path: "docs/img/x.png",
    });
    expect(html).toContain("data:image/png;base64,AAA");
  });

  it("inlines a relative SVG from its text content", async () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"></svg>';
    vi.mocked(invoke).mockResolvedValue({ kind: "text", text: svg });
    const html = await resolveMarkdownImages(
      wt,
      "readme.md",
      '<p><img src="design/logo/mark.svg" alt="mark"></p>',
    );
    expect(invoke).toHaveBeenCalledWith("read_worktree_file", {
      project: "corral",
      feature: "hulling",
      path: "design/logo/mark.svg",
    });
    expect(html).toContain("data:image/svg+xml;charset=utf-8,");
    expect(html).toContain(encodeURIComponent(svg));
  });

  it("leaves external images alone", async () => {
    const html = await resolveMarkdownImages(
      wt,
      "readme.md",
      '<img src="https://example.com/x.png">',
    );
    expect(invoke).not.toHaveBeenCalled();
    expect(html).toContain("https://example.com/x.png");
  });

  it("leaves a src in place when it can't be resolved", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("missing"));
    const html = await resolveMarkdownImages(
      wt,
      "readme.md",
      '<img src="missing.png">',
    );
    expect(html).toContain('src="missing.png"');
  });

  it("does not send a worktree-escaping path to the backend", async () => {
    await resolveMarkdownImages(
      wt,
      "readme.md",
      '<img src="../../etc/passwd">',
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});
