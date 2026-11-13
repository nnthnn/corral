import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { CheckRun, MergePrResult, PrComment, Worktree } from "../types";
import {
  addCheckToChat,
  addCommentsToChat,
  checksAreBlocking,
  checksArePending,
  checksBlockedNote,
  fetchPrChecks,
  fetchPrDetails,
  mergePr,
  summarizeChecks,
} from "./checks";
import * as data from "./data";
import * as toasts from "./toasts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./data", () => ({ loadAll: vi.fn() }));

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

const passCheck: CheckRun = {
  name: "build",
  workflow: "ci.yml",
  bucket: "pass",
  url: "https://x/1",
};
const failCheck: CheckRun = { ...passCheck, name: "test", bucket: "fail" };
const pendingCheck: CheckRun = {
  ...passCheck,
  name: "lint",
  bucket: "pending",
};
const skippingCheck: CheckRun = {
  ...passCheck,
  name: "docs",
  bucket: "skipping",
};
const cancelCheck: CheckRun = {
  ...passCheck,
  name: "deploy",
  bucket: "cancel",
};

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(data.loadAll).mockReset().mockResolvedValue(undefined);
});

describe("checksAreBlocking", () => {
  it("is false for undefined or empty", () => {
    expect(checksAreBlocking(undefined)).toBe(false);
    expect(checksAreBlocking([])).toBe(false);
  });
  it("is false when everything is pass or skipping", () => {
    expect(checksAreBlocking([passCheck, skippingCheck])).toBe(false);
  });
  it("is true when anything is failing", () => {
    expect(checksAreBlocking([passCheck, failCheck])).toBe(true);
  });
  it("is true when anything is still pending", () => {
    expect(checksAreBlocking([passCheck, pendingCheck])).toBe(true);
  });
});

describe("checksArePending", () => {
  it("is false for undefined, empty, or all-resolved", () => {
    expect(checksArePending(undefined)).toBe(false);
    expect(checksArePending([passCheck, failCheck])).toBe(false);
  });
  it("is true when anything is pending", () => {
    expect(checksArePending([passCheck, pendingCheck])).toBe(true);
  });
});

describe("summarizeChecks", () => {
  it("counts total, pending, and failed (fail + cancel)", () => {
    expect(
      summarizeChecks([
        passCheck,
        pendingCheck,
        pendingCheck,
        failCheck,
        cancelCheck,
        skippingCheck,
      ]),
    ).toEqual({ total: 6, pending: 2, failed: 2 });
  });

  it("is green with a zero total for an empty list", () => {
    expect(summarizeChecks([])).toEqual({ total: 0, pending: 0, failed: 0 });
  });
});

describe("checksBlockedNote", () => {
  it("says waiting only when every blocking check is still pending", () => {
    expect(checksBlockedNote([passCheck, pendingCheck])).toBe(
      "Waiting on checks…",
    );
  });
  it("reports a failure even when another check is still pending", () => {
    expect(checksBlockedNote([pendingCheck, failCheck])).toBe("Checks failed");
  });
  it("reports a cancellation distinctly from a failure", () => {
    expect(checksBlockedNote([cancelCheck, pendingCheck])).toBe(
      "Checks cancelled",
    );
  });
});

describe("fetchPrChecks", () => {
  it("fetches and caches checks by wtId, returning an awaitable promise resolving true on success", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([passCheck]);
    const result = fetchPrChecks(wt);
    expect(result).toBeInstanceOf(Promise);
    expect(await result).toBe(true);
    expect(invoke).toHaveBeenCalledWith("pr_checks", {
      project: "corral",
      feature: "hulling",
    });
    expect(useStore.getState().checksByWtId.get("corral/hulling")).toEqual([
      passCheck,
    ]);
  });

  it("does not show an error toast when the fetch fails — this runs automatically, not from a user action", async () => {
    const showErrorSpy = vi.spyOn(toasts, "showError");
    vi.mocked(invoke).mockRejectedValueOnce(new Error("network error"));
    const result = fetchPrChecks(wt);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalled());
    // A caller that does care (e.g. `PrPane`'s manual refresh) can still
    // tell the fetch failed off the resolved value.
    expect(await result).toBe(false);
    expect(showErrorSpy).not.toHaveBeenCalled();
  });
});

describe("fetchPrDetails", () => {
  it("fetches and caches PR details by wtId, returning an awaitable promise resolving true on success", async () => {
    const details = {
      title: "t",
      body: "b",
      author: "octocat",
      review_decision: "REVIEW_REQUIRED",
      comments: [],
    };
    vi.mocked(invoke).mockResolvedValueOnce(details);
    const result = fetchPrDetails(wt);
    expect(result).toBeInstanceOf(Promise);
    expect(await result).toBe(true);
    expect(useStore.getState().prDetailsByWtId.get("corral/hulling")).toEqual(
      details,
    );
  });

  it("treats a null result (no PR yet) as success without caching anything", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    const result = fetchPrDetails(wt);
    expect(await result).toBe(true);
    expect(useStore.getState().prDetailsByWtId.get("corral/hulling")).toBe(
      undefined,
    );
  });

  it("does not show an error toast when the fetch fails — this runs automatically, not from a user action", async () => {
    const showErrorSpy = vi.spyOn(toasts, "showError");
    vi.mocked(invoke).mockRejectedValueOnce(new Error("network error"));
    const result = fetchPrDetails(wt);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalled());
    // A caller that does care (e.g. `PrPane`'s manual refresh) can still
    // tell the fetch failed off the resolved value.
    expect(await result).toBe(false);
    expect(showErrorSpy).not.toHaveBeenCalled();
  });
});

describe("mergePr", () => {
  it("guards against a double-invoke while already merging", async () => {
    useStore.setState({ mergingPrIds: new Set(["corral/hulling"]) });
    await mergePr(wt);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("calls merge_pr, reloads on success, and clears the in-flight guard", async () => {
    const merged: MergePrResult = { outcome: "merged" };
    vi.mocked(invoke).mockResolvedValueOnce(merged);
    await mergePr(wt);
    expect(invoke).toHaveBeenCalledWith("merge_pr", {
      project: "corral",
      feature: "hulling",
    });
    expect(useStore.getState().mergingPrIds.has("corral/hulling")).toBe(false);
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("surfaces a rejection without throwing", async () => {
    const rejected: MergePrResult = {
      outcome: "rejected",
      reason: "required check missing",
    };
    vi.mocked(invoke).mockResolvedValueOnce(rejected);
    await expect(mergePr(wt)).resolves.toBeUndefined();
    expect(useStore.getState().mergingPrIds.has("corral/hulling")).toBe(false);
  });
});

describe("addCheckToChat", () => {
  it("fetches the failing log, attaches it as a file, and pastes a prompt referencing it", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce("line1\nline2")
      .mockResolvedValueOnce("/tmp/corral-attach-1.txt");
    const pasteToActive = vi.fn();
    useStore.setState({
      sessionMgr: { pasteToActive } as never,
      activeTab: "pr",
    });
    await addCheckToChat(wt, failCheck);
    expect(invoke).toHaveBeenCalledWith("check_log", {
      project: "corral",
      feature: "hulling",
      checkUrl: failCheck.url,
    });
    expect(invoke).toHaveBeenCalledWith("save_text_attachment", {
      text: "line1\nline2",
    });
    expect(pasteToActive).toHaveBeenCalledWith(
      expect.stringContaining('Check "test" failed on this branch'),
    );
    expect(pasteToActive).toHaveBeenCalledWith(
      expect.stringContaining("/tmp/corral-attach-1.txt"),
    );
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("falls back to pasting the log inline if the attachment write fails", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce("line1\nline2")
      .mockRejectedValueOnce(new Error("disk full"));
    const pasteToActive = vi.fn();
    useStore.setState({
      sessionMgr: { pasteToActive } as never,
      activeTab: "pr",
    });
    await addCheckToChat(wt, failCheck);
    expect(pasteToActive).toHaveBeenCalledWith(
      expect.stringContaining("line1\nline2"),
    );
    expect(useStore.getState().activeTab).toBe("agent");
  });
});

describe("addCommentsToChat", () => {
  it("does nothing for an empty list", () => {
    const pasteToActive = vi.fn();
    useStore.setState({
      sessionMgr: { pasteToActive } as never,
      activeTab: "pr",
    });
    addCommentsToChat([]);
    expect(pasteToActive).not.toHaveBeenCalled();
    expect(useStore.getState().activeTab).toBe("pr");
  });

  it("pastes a single comment with an instruction prefix", () => {
    const pasteToActive = vi.fn();
    useStore.setState({
      sessionMgr: { pasteToActive } as never,
      activeTab: "pr",
    });
    const comment: PrComment = {
      author: "github-actions",
      body: "CI note",
      created_at: "2026-07-15T00:00:00Z",
      url: "https://x/c1",
    };
    addCommentsToChat([comment]);
    expect(pasteToActive).toHaveBeenCalledWith(
      expect.stringContaining("PR comment from github-actions"),
    );
    expect(pasteToActive).toHaveBeenCalledWith(
      expect.stringContaining("CI note"),
    );
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("concatenates multiple comments into one paste", () => {
    const pasteToActive = vi.fn();
    useStore.setState({ sessionMgr: { pasteToActive } as never });
    const comments: PrComment[] = [
      {
        author: "alice",
        body: "first",
        created_at: "2026-07-15T00:00:00Z",
        url: "https://x/1",
      },
      {
        author: "bob",
        body: "second",
        created_at: "2026-07-15T00:01:00Z",
        url: "https://x/2",
      },
    ];
    addCommentsToChat(comments);
    expect(pasteToActive).toHaveBeenCalledTimes(1);
    const [text] = pasteToActive.mock.calls[0];
    expect(text).toContain("alice");
    expect(text).toContain("first");
    expect(text).toContain("bob");
    expect(text).toContain("second");
  });
});
