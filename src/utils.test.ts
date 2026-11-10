import { describe, expect, it } from "vitest";
import type { GitStatus, PrInfo, Worktree } from "./types";
import {
  adjacentWorktreeId,
  clampWidth,
  deriveLifecycleState,
  formatBytes,
  highlightMatch,
  isFullyMerged,
  moveProject,
  orderProjects,
  relativeTime,
  reorderProjects,
  rightSlotIndicator,
  sessionId,
  shortenPath,
  splitRenamedPath,
  summarizeText,
  worktreeTitle,
  wtId,
} from "./utils";

function worktree(status: GitStatus | null): Worktree {
  return {
    project: "corral",
    feature: "churning",
    slug: "churning",
    branch: "feat/churning",
    path: "/home/alice/source/worktrees/corral/churning",
    status,
    has_resumable_session: false,
    note: "",
  };
}

function gitStatus(overrides: Partial<GitStatus> = {}): GitStatus {
  return {
    branch: "feat/churning",
    ahead: 0,
    behind: 0,
    staged: 0,
    modified: 0,
    untracked: 0,
    added: 0,
    removed: 0,
    conflicts: 0,
    commits_ahead_of_base: 0,
    merged_into_base: false,
    conflicts_with_base: false,
    has_remote: false,
    last_commit_message: "",
    last_commit_time: 0,
    base_added: 0,
    base_removed: 0,
    ...overrides,
  };
}

describe("sessionId / wtId", () => {
  it("joins project and feature with a slash", () => {
    expect(sessionId("corral", "churning")).toBe("corral/churning");
  });

  it("derives a worktree's id from its project and feature", () => {
    expect(wtId(worktree(null))).toBe("corral/churning");
  });
});

describe("worktreeTitle", () => {
  const baseWt = worktree(null);

  it("returns the title when set", () => {
    expect(
      worktreeTitle({
        ...baseWt,
        title: "Fix Login Timeout",
        slug: "fix-login-timeout",
      }),
    ).toBe("Fix Login Timeout");
  });

  it("falls back to slug when title is unset", () => {
    expect(worktreeTitle({ ...baseWt, title: "", slug: "churning" })).toBe(
      "churning",
    );
  });

  it("falls back to slug when title is absent", () => {
    const { title, ...withoutTitle } = { ...baseWt, slug: "churning" };
    expect(worktreeTitle(withoutTitle)).toBe("churning");
  });
});

describe("highlightMatch", () => {
  it("returns the escaped text unchanged when there's no needle", () => {
    expect(highlightMatch("lint-format-test", "")).toBe("lint-format-test");
  });

  it("returns the escaped text unchanged when there's no match", () => {
    expect(highlightMatch("lint-format-test", "xyz")).toBe("lint-format-test");
  });

  it("wraps the first match in a <mark>, case-insensitively against the text", () => {
    // `needle` is expected pre-lowercased by the caller (the sidebar filter
    // always stores it lowercased); matching is case-insensitive on `text`.
    expect(highlightMatch("Lint-Format-Test", "format")).toBe(
      'Lint-<mark class="match">Format</mark>-Test',
    );
  });

  it("escapes the surrounding text", () => {
    expect(highlightMatch("<b>corral</b>", "corral")).toBe(
      '&lt;b&gt;<mark class="match">corral</mark>&lt;/b&gt;',
    );
  });
});

describe("splitRenamedPath", () => {
  it("returns the shared directory and both filenames when the directory is unchanged", () => {
    expect(
      splitRenamedPath("src/components/old.tsx", "src/components/new.tsx"),
    ).toEqual({
      dir: "src/components/",
      oldName: "old.tsx",
      newName: "new.tsx",
    });
  });

  it("returns an empty dir when neither path has one", () => {
    expect(splitRenamedPath("old.tsx", "new.tsx")).toEqual({
      dir: "",
      oldName: "old.tsx",
      newName: "new.tsx",
    });
  });

  it("returns null when the directory itself changed", () => {
    expect(
      splitRenamedPath("src/lib/oldutils.ts", "src/utils/newHelpers.ts"),
    ).toBeNull();
  });

  it("returns null when a path gains or loses a directory", () => {
    expect(splitRenamedPath("old.tsx", "src/new.tsx")).toBeNull();
  });
});

function status(overrides: Partial<GitStatus> = {}): GitStatus {
  return {
    branch: "feat/hulling",
    ahead: 0,
    behind: 0,
    staged: 0,
    modified: 0,
    untracked: 0,
    added: 0,
    removed: 0,
    conflicts: 0,
    commits_ahead_of_base: 0,
    merged_into_base: false,
    conflicts_with_base: false,
    has_remote: false,
    last_commit_message: "",
    last_commit_time: 0,
    base_added: 0,
    base_removed: 0,
    ...overrides,
  };
}

function pr(overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 1,
    url: "https://github.com/x/y/pull/1",
    state: "open",
    is_draft: false,
    ...overrides,
  };
}

describe("deriveLifecycleState", () => {
  it("returns 'broken' when there's no git status (unreadable worktree directory)", () => {
    expect(deriveLifecycleState(null, null, false)).toBe("broken");
  });

  it("returns new for a fresh, untouched branch", () => {
    expect(deriveLifecycleState(status(), null, false)).toBe("new");
  });

  it("returns in-progress when dirty, even with no PR/commits", () => {
    expect(deriveLifecycleState(status({ modified: 1 }), null, false)).toBe(
      "in-progress",
    );
  });

  it("returns committed once ahead of base with no PR yet", () => {
    expect(
      deriveLifecycleState(status({ commits_ahead_of_base: 2 }), null, false),
    ).toBe("committed");
  });

  it("returns merged from local merged_into_base when no PR info exists", () => {
    expect(
      deriveLifecycleState(status({ merged_into_base: true }), null, false),
    ).toBe("merged");
  });

  it("returns merged via the justMerged bridge when merged_into_base can't tell yet", () => {
    expect(deriveLifecycleState(status(), null, true)).toBe("merged");
  });

  it("does not use the justMerged bridge if commits are ahead of base", () => {
    expect(
      deriveLifecycleState(status({ commits_ahead_of_base: 1 }), null, true),
    ).toBe("committed");
  });

  it("returns pr_draft for a draft PR", () => {
    expect(
      deriveLifecycleState(
        status(),
        pr({ state: "open", is_draft: true }),
        false,
      ),
    ).toBe("pr_draft");
  });

  it("returns pr_open for a non-draft open PR", () => {
    expect(
      deriveLifecycleState(
        status(),
        pr({ state: "open", is_draft: false }),
        false,
      ),
    ).toBe("pr_open");
  });

  it("returns pr_closed for a closed (not merged) PR", () => {
    expect(deriveLifecycleState(status(), pr({ state: "closed" }), false)).toBe(
      "pr_closed",
    );
  });

  it("returns merged when GitHub reports the PR merged, regardless of local ancestry", () => {
    // Simulates a squash merge: local commits_ahead_of_base stays > 0
    // since the squash commit's hash never lands in this branch's history,
    // but GitHub's PR state is authoritative.
    expect(
      deriveLifecycleState(
        status({ commits_ahead_of_base: 3, merged_into_base: false }),
        pr({ state: "merged" }),
        false,
      ),
    ).toBe("merged");
  });

  it("dirty overrides merged even when GitHub reports the PR merged", () => {
    expect(
      deriveLifecycleState(
        status({ modified: 1 }),
        pr({ state: "merged" }),
        false,
      ),
    ).toBe("in-progress");
  });

  it("dirty overrides pr_open", () => {
    expect(
      deriveLifecycleState(status({ staged: 1 }), pr({ state: "open" }), false),
    ).toBe("in-progress");
  });

  it("pr_closed takes priority over local commits_ahead_of_base", () => {
    expect(
      deriveLifecycleState(
        status({ commits_ahead_of_base: 1 }),
        pr({ state: "closed" }),
        false,
      ),
    ).toBe("pr_closed");
  });
});

describe("isFullyMerged", () => {
  it("is true exactly when deriveLifecycleState returns merged", () => {
    expect(isFullyMerged(status({ merged_into_base: true }), null, false)).toBe(
      true,
    );
    expect(isFullyMerged(status(), pr({ state: "merged" }), false)).toBe(true);
  });

  it("is false when dirty, even if merged_into_base is true", () => {
    expect(
      isFullyMerged(
        status({ merged_into_base: true, modified: 1 }),
        null,
        false,
      ),
    ).toBe(false);
  });

  it("is false for an open or closed PR", () => {
    expect(isFullyMerged(status(), pr({ state: "open" }), false)).toBe(false);
    expect(isFullyMerged(status(), pr({ state: "closed" }), false)).toBe(false);
  });
});

describe("shortenPath", () => {
  it("replaces a leading home directory with ~", () => {
    expect(
      shortenPath(
        "/home/alice/source/worktrees/corral/churning",
        "/home/alice",
      ),
    ).toBe("~/source/worktrees/corral/churning");
  });

  it("leaves the path unchanged when homeDir is undefined", () => {
    expect(shortenPath("/home/alice/source/corral", undefined)).toBe(
      "/home/alice/source/corral",
    );
  });

  it("leaves the path unchanged when it isn't under homeDir", () => {
    expect(shortenPath("/var/log/corral.log", "/home/alice")).toBe(
      "/var/log/corral.log",
    );
  });

  it("does not shorten a sibling that merely shares the home prefix", () => {
    expect(shortenPath("/home/aliceson/project", "/home/alice")).toBe(
      "/home/aliceson/project",
    );
  });

  it("shortens the exact home directory to ~", () => {
    expect(shortenPath("/home/alice", "/home/alice")).toBe("~");
  });
});

describe("clampWidth", () => {
  it("returns the value unchanged when within bounds", () => {
    expect(clampWidth(300, 200, 500)).toBe(300);
  });

  it("clamps to the minimum", () => {
    expect(clampWidth(50, 200, 500)).toBe(200);
  });

  it("clamps to the maximum", () => {
    expect(clampWidth(900, 200, 500)).toBe(500);
  });
});

describe("summarizeText", () => {
  it("strips headers, emphasis, inline code, and links", () => {
    expect(
      summarizeText(
        "## Fixed it\n\nAdded a **retry with backoff** in `refreshToken()`, see [the PR](https://example.com).",
      ),
    ).toBe(
      "Fixed it Added a retry with backoff in refreshToken(), see the PR.",
    );
  });

  it("drops fenced code blocks entirely", () => {
    expect(
      summarizeText("Here's the fix:\n\n```ts\nconst x = 1;\n```\n\nAll set."),
    ).toBe("Here's the fix: All set.");
  });

  it("collapses newlines and repeated whitespace to single spaces", () => {
    expect(summarizeText("Line one.\n\n\nLine   two.")).toBe(
      "Line one. Line two.",
    );
  });

  it("leaves short plain text unchanged", () => {
    expect(summarizeText("Fixed the bug.")).toBe("Fixed the bug.");
  });

  it("hard-truncates long text with an ellipsis at maxLen", () => {
    const long = "a".repeat(200);
    const result = summarizeText(long, 20);
    expect(result).toHaveLength(20);
    expect(result.endsWith("…")).toBe(true);
  });
});

describe("orderProjects", () => {
  it("keeps the stored order for listed projects", () => {
    expect(orderProjects(["a", "b", "c"], ["c", "a", "b"])).toEqual([
      "c",
      "a",
      "b",
    ]);
  });

  it("appends new projects (not in the stored order) alphabetically after", () => {
    expect(orderProjects(["a", "b", "c"], ["b"])).toEqual(["b", "a", "c"]);
  });

  it("falls back to alphabetical when there's no stored order", () => {
    expect(orderProjects(["a", "b", "c"], [])).toEqual(["a", "b", "c"]);
  });

  it("drops stored entries for projects that no longer exist", () => {
    expect(orderProjects(["a", "b"], ["gone", "b", "a"])).toEqual(["b", "a"]);
  });
});

describe("adjacentWorktreeId", () => {
  function wt(project: string, feature: string): Worktree {
    return {
      project,
      feature,
      slug: feature,
      branch: `feat/${feature}`,
      path: `/path/${feature}`,
      status: null,
      has_resumable_session: false,
      note: "",
    };
  }

  it("picks the next worktree in the same project", () => {
    const worktrees = [wt("corral", "a"), wt("corral", "b"), wt("corral", "c")];
    expect(adjacentWorktreeId(worktrees, ["corral"], [], "corral/b")).toBe(
      "corral/c",
    );
  });

  it("falls back to the previous worktree when the removed one was last", () => {
    const worktrees = [wt("corral", "a"), wt("corral", "b"), wt("corral", "c")];
    expect(adjacentWorktreeId(worktrees, ["corral"], [], "corral/c")).toBe(
      "corral/b",
    );
  });

  it("crosses into the next project when the removed worktree was its project's last", () => {
    const worktrees = [wt("corral", "a"), wt("other", "x")];
    expect(
      adjacentWorktreeId(worktrees, ["corral", "other"], [], "corral/a"),
    ).toBe("other/x");
  });

  it("respects a custom project order", () => {
    const worktrees = [wt("corral", "a"), wt("other", "x")];
    expect(
      adjacentWorktreeId(
        worktrees,
        ["corral", "other"],
        ["other", "corral"],
        "other/x",
      ),
    ).toBe("corral/a");
  });

  it("returns null when the removed worktree was the only one", () => {
    const worktrees = [wt("corral", "a")];
    expect(
      adjacentWorktreeId(worktrees, ["corral"], [], "corral/a"),
    ).toBeNull();
  });

  it("returns null when the id isn't found", () => {
    const worktrees = [wt("corral", "a")];
    expect(
      adjacentWorktreeId(worktrees, ["corral"], [], "corral/gone"),
    ).toBeNull();
  });
});

describe("moveProject", () => {
  it("moves a project up one spot", () => {
    expect(moveProject(["a", "b", "c"], "b", -1)).toEqual(["b", "a", "c"]);
  });

  it("moves a project down one spot", () => {
    expect(moveProject(["a", "b", "c"], "b", 1)).toEqual(["a", "c", "b"]);
  });

  it("is a no-op moving the first project up", () => {
    expect(moveProject(["a", "b", "c"], "a", -1)).toEqual(["a", "b", "c"]);
  });

  it("is a no-op moving the last project down", () => {
    expect(moveProject(["a", "b", "c"], "c", 1)).toEqual(["a", "b", "c"]);
  });

  it("is a no-op for an unknown project", () => {
    expect(moveProject(["a", "b", "c"], "z", 1)).toEqual(["a", "b", "c"]);
  });
});

describe("reorderProjects", () => {
  it("moves the dragged project before the target", () => {
    expect(reorderProjects(["a", "b", "c"], "c", "a", "before")).toEqual([
      "c",
      "a",
      "b",
    ]);
  });

  it("moves the dragged project after the target", () => {
    expect(reorderProjects(["a", "b", "c"], "a", "c", "after")).toEqual([
      "b",
      "c",
      "a",
    ]);
  });

  it("is a no-op when dragged and target are the same", () => {
    expect(reorderProjects(["a", "b", "c"], "b", "b", "after")).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("is a no-op for an unknown target", () => {
    expect(reorderProjects(["a", "b", "c"], "a", "z", "after")).toEqual([
      "a",
      "b",
      "c",
    ]);
  });
});

describe("rightSlotIndicator", () => {
  it("prioritizes a running Run command over everything else, including attention", () => {
    const status = gitStatus({ staged: 1, ahead: 2 });
    expect(rightSlotIndicator(status, true, true)).toEqual({
      kind: "running",
    });
  });

  it("reports running even with no git status at all", () => {
    expect(rightSlotIndicator(null, false, true)).toEqual({
      kind: "running",
    });
  });

  it("prioritizes attention over everything else when nothing is running", () => {
    const status = gitStatus({ staged: 1, ahead: 2 });
    expect(rightSlotIndicator(status, true, false)).toEqual({
      kind: "attention",
    });
  });

  it("falls back to uncommitted diff stats when dirty with no attention", () => {
    const status = gitStatus({
      staged: 1,
      modified: 2,
      untracked: 3,
      added: 12,
      removed: 4,
      ahead: 1,
    });
    expect(rightSlotIndicator(status, false, false)).toEqual({
      kind: "diff",
      added: 12,
      removed: 4,
    });
  });

  it("falls back to branch-vs-base diff stats when clean but ahead of base", () => {
    const status = gitStatus({ base_added: 40, base_removed: 5, ahead: 1 });
    expect(rightSlotIndicator(status, false, false)).toEqual({
      kind: "diff",
      added: 40,
      removed: 5,
    });
  });

  it("falls back to ahead/behind when clean with no diff stats", () => {
    const status = gitStatus({ ahead: 2, behind: 1 });
    expect(rightSlotIndicator(status, false, false)).toEqual({
      kind: "sync",
      ahead: 2,
      behind: 1,
    });
  });

  it("returns none when clean and in sync", () => {
    const status = gitStatus({});
    expect(rightSlotIndicator(status, false, false)).toEqual({ kind: "none" });
  });

  it("returns none when there's no git status at all", () => {
    expect(rightSlotIndicator(null, false, false)).toEqual({ kind: "none" });
  });

  it("still reports attention with no git status", () => {
    expect(rightSlotIndicator(null, true, false)).toEqual({
      kind: "attention",
    });
  });

  it("reports conflict above diff/sync signals", () => {
    const status = gitStatus({
      conflicts_with_base: true,
      ahead: 1,
      base_added: 3,
      base_removed: 1,
    });
    expect(rightSlotIndicator(status, false, false)).toEqual({
      kind: "conflict",
    });
  });

  it("still reports attention/running above conflict", () => {
    const status = gitStatus({ conflicts_with_base: true });
    expect(rightSlotIndicator(status, true, false)).toEqual({
      kind: "attention",
    });
    expect(rightSlotIndicator(status, false, true)).toEqual({
      kind: "running",
    });
  });

  it("reports failed CI above conflict and diff stats", () => {
    const status = gitStatus({
      conflicts_with_base: true,
      base_added: 3,
      base_removed: 1,
    });
    expect(
      rightSlotIndicator(status, false, false, {
        total: 3,
        pending: 0,
        failed: 1,
      }),
    ).toEqual({ kind: "checks-failed" });
  });

  it("reports failed CI below attention and a running Run command", () => {
    const checks = { total: 3, pending: 0, failed: 1 };
    const status = gitStatus({});
    expect(rightSlotIndicator(status, true, false, checks)).toEqual({
      kind: "attention",
    });
    expect(rightSlotIndicator(status, false, true, checks)).toEqual({
      kind: "running",
    });
  });

  it("reports running CI above diff/sync but below conflict", () => {
    const checks = { total: 3, pending: 2, failed: 0 };
    expect(
      rightSlotIndicator(
        gitStatus({ base_added: 9, base_removed: 2 }),
        false,
        false,
        checks,
      ),
    ).toEqual({ kind: "checks-running" });
    expect(
      rightSlotIndicator(
        gitStatus({ conflicts_with_base: true }),
        false,
        false,
        checks,
      ),
    ).toEqual({ kind: "conflict" });
  });

  it("falls through when the PR has no checks at all", () => {
    const status = gitStatus({ ahead: 2 });
    expect(
      rightSlotIndicator(status, false, false, {
        total: 0,
        pending: 0,
        failed: 0,
      }),
    ).toEqual({ kind: "sync", ahead: 2, behind: 0 });
  });
});

describe("relativeTime", () => {
  const now = new Date("2026-07-14T00:00:00Z").getTime();

  it("formats a time in the past as days ago", () => {
    const twoDaysAgo = now / 1000 - 2 * 24 * 60 * 60;
    expect(relativeTime(twoDaysAgo, now)).toBe("2 days ago");
  });

  it("formats a time in the past as hours ago", () => {
    const threeHoursAgo = now / 1000 - 3 * 60 * 60;
    expect(relativeTime(threeHoursAgo, now)).toBe("3 hours ago");
  });

  it("falls back to minutes for anything under an hour", () => {
    const thirtySecondsAgo = now / 1000 - 30;
    expect(relativeTime(thirtySecondsAgo, now)).toBe("this minute");
  });
});

describe("formatBytes", () => {
  it("formats gigabytes with two decimals", () => {
    expect(formatBytes(1.01 * 1024 * 1024 * 1024)).toBe("1.01 GB");
  });

  it("formats megabytes with one decimal", () => {
    expect(formatBytes(999.0 * 1024 * 1024)).toBe("999.0 MB");
  });

  it("formats kilobytes as a rounded whole number", () => {
    expect(formatBytes(512 * 1024)).toBe("512 KB");
  });

  it("formats sub-kilobyte values in bytes", () => {
    expect(formatBytes(512)).toBe("512 B");
  });
});
