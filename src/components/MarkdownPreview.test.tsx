import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Worktree } from "../types";
import { MarkdownPreview } from "./MarkdownPreview";
import styles from "./MarkdownPreview.module.css";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

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
  vi.mocked(openUrl).mockReset();
});

describe("MarkdownPreview", () => {
  it("renders headings and links", () => {
    render(
      <MarkdownPreview
        wt={wt}
        path="README.md"
        text={"# Hello\n\n[site](https://example.com)"}
      />,
    );
    expect(screen.getByRole("heading", { name: "Hello" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "site" })).toHaveAttribute(
      "href",
      "https://example.com",
    );
  });

  it("applies the compact size class and the caller's className", () => {
    const { container } = render(
      <MarkdownPreview
        wt={wt}
        path="README.md"
        text="# Hi"
        compact
        className="extra"
      />,
    );
    const body = container.firstElementChild;
    expect(body).toHaveClass("markdown-body");
    expect(body).toHaveClass(styles.compact);
    expect(body).toHaveClass("extra");
  });

  it("strips scripts from raw HTML", () => {
    const { container } = render(
      <MarkdownPreview
        wt={wt}
        path="README.md"
        text={"<script>alert(1)</script>"}
      />,
    );
    expect(container.querySelector("script")).toBeNull();
  });

  it("opens http(s) links in the system browser instead of navigating", () => {
    render(
      <MarkdownPreview
        wt={wt}
        path="README.md"
        text={"[site](https://example.com)"}
      />,
    );
    fireEvent.click(screen.getByRole("link", { name: "site" }));
    expect(openUrl).toHaveBeenCalledWith("https://example.com");
  });

  it("leaves relative links inert", () => {
    render(
      <MarkdownPreview wt={wt} path="README.md" text={"[other](./other.md)"} />,
    );
    fireEvent.click(screen.getByRole("link", { name: "other" }));
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("inlines a relative image read from the worktree", async () => {
    vi.mocked(invoke).mockResolvedValue({
      kind: "image",
      mime: "image/png",
      data: "AAA",
    });
    render(
      <MarkdownPreview
        wt={wt}
        path="docs/readme.md"
        text={"![shot](img/shot.png)"}
      />,
    );
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "shot" })).toHaveAttribute(
        "src",
        "data:image/png;base64,AAA",
      ),
    );
    expect(invoke).toHaveBeenCalledWith("read_worktree_file", {
      project: "corral",
      feature: "hulling",
      path: "docs/img/shot.png",
    });
  });
});
