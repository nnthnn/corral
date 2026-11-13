import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as checks from "../lib/checks";
import { useStore } from "../store";
import type { CheckRun, PrDetails, Worktree } from "../types";
import { PrPane } from "./PrPane";

vi.mock("../lib/checks", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/checks")>("../lib/checks");
  return {
    ...actual,
    // Default to a successful fetch (`true`) so tests unrelated to error
    // handling don't unexpectedly trip the refresh-failed banner.
    fetchPrChecks: vi.fn().mockResolvedValue(true),
    fetchPrDetails: vi.fn().mockResolvedValue(true),
    addCheckToChat: vi.fn(),
    addCommentsToChat: vi.fn(),
    markPrReady: vi.fn().mockResolvedValue(undefined),
  };
});

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

const details: PrDetails = {
  title: "Add feature",
  body: "## Summary\n- did a thing",
  author: "octocat",
  review_decision: "REVIEW_REQUIRED",
  comments: [
    {
      author: "github-actions",
      body: "CI note",
      created_at: "2026-07-15T00:00:00Z",
      url: "https://x/c1",
    },
  ],
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

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  // Default to a successful fetch (`true`) so tests unrelated to error
  // handling don't unexpectedly trip the refresh-failed banner — `mockReset`
  // wipes the factory-level default above, so it's re-applied here every
  // test; individual tests override with `mockResolvedValueOnce(false)`.
  vi.mocked(checks.fetchPrChecks).mockReset().mockResolvedValue(true);
  vi.mocked(checks.fetchPrDetails).mockReset().mockResolvedValue(true);
  vi.mocked(checks.addCheckToChat).mockReset();
  vi.mocked(checks.addCommentsToChat).mockReset();
  vi.mocked(checks.markPrReady).mockReset().mockResolvedValue(undefined);
});

describe("PrPane", () => {
  it("fetches PR details and checks on mount", () => {
    render(<PrPane wt={wt} />);
    expect(checks.fetchPrDetails).toHaveBeenCalledWith(wt);
    expect(checks.fetchPrChecks).toHaveBeenCalledWith(wt);
  });

  it("does not fetch while its tab is inactive, then fetches when activated", () => {
    const { rerender } = render(<PrPane wt={wt} active={false} />);
    expect(checks.fetchPrDetails).not.toHaveBeenCalled();
    expect(checks.fetchPrChecks).not.toHaveBeenCalled();

    rerender(<PrPane wt={wt} active={true} />);
    expect(checks.fetchPrDetails).toHaveBeenCalledWith(wt);
    expect(checks.fetchPrChecks).toHaveBeenCalledWith(wt);
  });

  it("renders the PR title and body once loaded", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Add feature")).toBeInTheDocument();
  });

  it("keeps checks collapsed until the summary card's Checks segment is opened", () => {
    useStore.setState({
      checksByWtId: new Map([
        ["corral/hulling", [passCheck, failCheck, pendingCheck]],
      ]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.queryByText("Passed 1")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Checks" }));
    expect(screen.getByText("Passed 1")).toBeInTheDocument();
    expect(screen.getByText("Failed 1")).toBeInTheDocument();
    expect(screen.getByText("In progress 1")).toBeInTheDocument();
    expect(screen.getByText("build")).toBeInTheDocument();
    expect(screen.getByText("test")).toBeInTheDocument();
    expect(screen.getByText("lint")).toBeInTheDocument();
  });

  it("shows the checks pass ratio in the summary card", () => {
    useStore.setState({
      checksByWtId: new Map([["corral/hulling", [passCheck, failCheck]]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("1/2 passed")).toBeInTheDocument();
  });

  it("shows a 'Checks running' indicator in the header while a check is pending", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 7, url: "https://x/pr/7", state: "open", is_draft: false },
        ],
      ]),
      checksByWtId: new Map([["corral/hulling", [pendingCheck]]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Checks running")).toBeInTheDocument();
  });

  it("hides the 'Checks running' indicator once every check resolves", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 7, url: "https://x/pr/7", state: "open", is_draft: false },
        ],
      ]),
      checksByWtId: new Map([["corral/hulling", [passCheck]]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.queryByText("Checks running")).not.toBeInTheDocument();
  });

  it("only shows an Add to chat button on failed checks once checks are open", () => {
    useStore.setState({
      checksByWtId: new Map([["corral/hulling", [passCheck, failCheck]]]),
    });
    render(<PrPane wt={wt} />);
    fireEvent.click(screen.getByRole("button", { name: "Checks" }));
    expect(screen.getAllByText("Add to chat")).toHaveLength(1);
  });

  it("calls addCheckToChat when a failed check's Add to chat is clicked", () => {
    useStore.setState({
      checksByWtId: new Map([["corral/hulling", [failCheck]]]),
    });
    render(<PrPane wt={wt} />);
    fireEvent.click(screen.getByRole("button", { name: "Checks" }));
    fireEvent.click(screen.getByText("Add to chat"));
    expect(checks.addCheckToChat).toHaveBeenCalledWith(wt, failCheck);
  });

  it("renders comment rows below the description", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
    });
    render(<PrPane wt={wt} />);
    // Comment bodies render as markdown, not raw preformatted text.
    expect(screen.getByText("CI note").tagName).toBe("P");
  });

  it("calls addCommentsToChat for a single comment's Add to chat", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
    });
    render(<PrPane wt={wt} />);
    fireEvent.click(screen.getByText("Add to chat"));
    expect(checks.addCommentsToChat).toHaveBeenCalledWith([
      details.comments[0],
    ]);
  });

  it("renders the PR body as an unheaded description card", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
    });
    render(<PrPane wt={wt} />);
    // The `## Summary` heading and `-` list are rendered as real elements.
    expect(
      screen.getByRole("heading", { name: "Summary" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/did a thing/)).toBeInTheDocument();
    // The description itself has no author/"opened this pull request" chrome.
    expect(
      screen.queryByText("opened this pull request"),
    ).not.toBeInTheDocument();
  });

  it("shows Approval required in the summary card when review is required and checks pass", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      checksByWtId: new Map([["corral/hulling", [passCheck]]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Approval required")).toBeInTheDocument();
    expect(screen.getByText("Review required")).toBeInTheDocument();
  });

  it("shows All checks have passed when no review is required", () => {
    useStore.setState({
      prDetailsByWtId: new Map([
        ["corral/hulling", { ...details, review_decision: "" }],
      ]),
      checksByWtId: new Map([["corral/hulling", [passCheck]]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("All checks have passed")).toBeInTheDocument();
    // No review required → no empty Approvals column.
    expect(
      screen.queryByRole("button", { name: "Approvals" }),
    ).not.toBeInTheDocument();
  });

  it("shows Ready to merge when there are no checks or required reviews", () => {
    useStore.setState({
      prDetailsByWtId: new Map([
        ["corral/hulling", { ...details, review_decision: "" }],
      ]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Ready to merge")).toBeInTheDocument();
    expect(
      screen.getByText("No required checks or reviews."),
    ).toBeInTheDocument();
  });

  it("shows Checks failed in the summary card when a check fails", () => {
    useStore.setState({
      prDetailsByWtId: new Map([
        ["corral/hulling", { ...details, review_decision: "" }],
      ]),
      checksByWtId: new Map([["corral/hulling", [failCheck]]]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Checks failed")).toBeInTheDocument();
    expect(screen.getByText("1 check did not pass.")).toBeInTheDocument();
  });

  it("Add all comments to chat sends every comment", () => {
    const twoComments: PrDetails = {
      ...details,
      comments: [
        ...details.comments,
        {
          author: "alice",
          body: "second",
          created_at: "2026-07-16T00:00:00Z",
          url: "https://x/2",
        },
      ],
    };
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", twoComments]]),
    });
    render(<PrPane wt={wt} />);
    fireEvent.click(screen.getByText("Add all comments to chat"));
    expect(checks.addCommentsToChat).toHaveBeenCalledWith(twoComments.comments);
  });

  it("shows the PR status badge derived from prInfoByWtId, preferring draft over open", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 1, url: "https://x/pr/1", state: "open", is_draft: true },
        ],
      ]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Draft")).toBeInTheDocument();
  });

  it("shows the PR number next to the title", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          {
            number: 42,
            url: "https://x/pr/42",
            state: "open",
            is_draft: false,
          },
        ],
      ]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("#42")).toBeInTheDocument();
  });

  it("shows a Ready for review button for a draft PR, and calls markPrReady", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 1, url: "https://x/pr/1", state: "open", is_draft: true },
        ],
      ]),
    });
    render(<PrPane wt={wt} />);
    fireEvent.click(screen.getByText("Ready for review"));
    expect(checks.markPrReady).toHaveBeenCalledWith(wt);
  });

  it("doesn't show a Ready for review button for a non-draft PR", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 1, url: "https://x/pr/1", state: "open", is_draft: false },
        ],
      ]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.queryByText("Ready for review")).not.toBeInTheDocument();
  });

  it("shows the PR status badge as Merged when the PR has merged", () => {
    useStore.setState({
      prDetailsByWtId: new Map([["corral/hulling", details]]),
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          {
            number: 1,
            url: "https://x/pr/1",
            state: "merged",
            is_draft: false,
          },
        ],
      ]),
    });
    render(<PrPane wt={wt} />);
    expect(screen.getByText("Merged")).toBeInTheDocument();
  });

  it("manual refresh re-fetches checks and details", async () => {
    render(<PrPane wt={wt} />);
    // Let the mount-time refresh() settle (mocked fetches resolve to
    // `true`) so the refresh button isn't disabled when we click it.
    await vi.waitFor(() =>
      expect(screen.getByTitle("Refresh")).not.toBeDisabled(),
    );
    vi.mocked(checks.fetchPrChecks).mockClear();
    vi.mocked(checks.fetchPrDetails).mockClear();
    fireEvent.click(screen.getByTitle("Refresh"));
    expect(checks.fetchPrChecks).toHaveBeenCalledWith(wt);
    expect(checks.fetchPrDetails).toHaveBeenCalledWith(wt);
  });

  it("shows a spinner and disables the refresh button while a manual refresh is in flight, then reverts", async () => {
    let resolveDetails!: (ok: boolean) => void;
    let resolveChecksFn!: (ok: boolean) => void;
    vi.mocked(checks.fetchPrDetails).mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveDetails = resolve;
      }),
    );
    vi.mocked(checks.fetchPrChecks).mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveChecksFn = resolve;
      }),
    );
    render(<PrPane wt={wt} />);

    // Mount-time refresh() call is in flight — button already shows the
    // spinner and is disabled.
    const refreshButton = screen.getByTitle("Refresh");
    expect(refreshButton).toBeDisabled();
    expect(
      refreshButton.querySelector("svg.lucide-refresh-cw"),
    ).not.toBeInTheDocument();
    expect(refreshButton.querySelector("svg")).toBeInTheDocument();

    resolveDetails(true);
    resolveChecksFn(true);
    await vi.waitFor(() => expect(refreshButton).not.toBeDisabled());
    expect(
      refreshButton.querySelector("svg.lucide-refresh-cw"),
    ).toBeInTheDocument();
  });

  it("shows an inline error banner when a refresh fails for a worktree with a known PR", async () => {
    useStore.setState({
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 1, url: "https://x/pr/1", state: "open", is_draft: false },
        ],
      ]),
    });
    vi.mocked(checks.fetchPrDetails).mockResolvedValueOnce(false);
    render(<PrPane wt={wt} />);
    expect(await screen.findByText(/Couldn't refresh/)).toBeInTheDocument();
  });

  it("clears the error banner once a later refresh succeeds", async () => {
    useStore.setState({
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 1, url: "https://x/pr/1", state: "open", is_draft: false },
        ],
      ]),
    });
    vi.mocked(checks.fetchPrDetails).mockResolvedValueOnce(false);
    render(<PrPane wt={wt} />);
    await screen.findByText(/Couldn't refresh/);

    vi.mocked(checks.fetchPrDetails).mockResolvedValueOnce(true);
    fireEvent.click(screen.getByTitle("Refresh"));
    await vi.waitFor(() =>
      expect(screen.queryByText(/Couldn't refresh/)).not.toBeInTheDocument(),
    );
  });

  it("never shows the error banner before any PR has been pushed, even if the fetch fails", async () => {
    // Regression test: opening/mounting the PR pane for a worktree that has
    // no PR yet always fails its fetch (there's nothing to fetch) — that's
    // normal, not a real error, and shouldn't alarm the user.
    vi.mocked(checks.fetchPrDetails).mockResolvedValue(false);
    vi.mocked(checks.fetchPrChecks).mockResolvedValue(false);
    render(<PrPane wt={wt} />);
    await vi.waitFor(() => expect(checks.fetchPrDetails).toHaveBeenCalled());
    expect(screen.queryByText(/Couldn't refresh/)).not.toBeInTheDocument();
  });
});
