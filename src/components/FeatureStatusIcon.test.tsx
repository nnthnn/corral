import { cleanup, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { FeatureStatus } from "../utils";
import { FeatureStatusIcon } from "./FeatureStatusIcon";

describe("FeatureStatusIcon", () => {
  it("labels a broken worktree as not a git repository", () => {
    const { getByTitle } = render(<FeatureStatusIcon status="broken" />);
    expect(
      getByTitle("Not a git repository — safe to remove"),
    ).toBeInTheDocument();
  });

  it("renders a labeled icon for each known status", () => {
    for (const status of [
      "new",
      "in-progress",
      "committed",
      "pr_draft",
      "pr_open",
      "pr_closed",
      "merged",
      "broken",
    ] as const) {
      const { getByTitle } = render(<FeatureStatusIcon status={status} />);
      expect(getByTitle(/^/i)).toBeTruthy();
      // render() binds queries to document.body by default (not just the
      // local container), so without cleanup between iterations the next
      // iteration's getByTitle sees every previously-rendered icon too and
      // throws "multiple elements found".
      cleanup();
    }
  });

  it("labels 'in-progress' as In progress", () => {
    const { getByTitle } = render(<FeatureStatusIcon status="in-progress" />);
    expect(getByTitle("In progress")).toBeInTheDocument();
  });

  it("labels 'pr_draft' as Draft PR", () => {
    const { getByTitle } = render(<FeatureStatusIcon status="pr_draft" />);
    expect(getByTitle("Draft PR")).toBeInTheDocument();
  });

  it("labels 'pr_open' as PR open", () => {
    const { getByTitle } = render(<FeatureStatusIcon status="pr_open" />);
    expect(getByTitle("PR open")).toBeInTheDocument();
  });

  it("labels 'pr_closed' as PR closed", () => {
    const { getByTitle } = render(<FeatureStatusIcon status="pr_closed" />);
    expect(getByTitle("PR closed")).toBeInTheDocument();
  });

  it("labels 'merged' as Merged", () => {
    const { getByTitle } = render(<FeatureStatusIcon status="merged" />);
    expect(getByTitle("Merged")).toBeInTheDocument();
  });

  it("renders the fixed color for each status", () => {
    const expected: Record<FeatureStatus, string> = {
      new: "text-muted",
      "in-progress": "text-muted",
      committed: "text-muted",
      pr_draft: "text-muted",
      pr_open: "text-success",
      pr_closed: "text-danger",
      merged: "text-decorative",
      broken: "text-danger",
    };
    for (const status of Object.keys(expected) as FeatureStatus[]) {
      const { container } = render(<FeatureStatusIcon status={status} />);
      expect(container.querySelector("svg")).toHaveClass(expected[status]);
      cleanup();
    }
  });
});
