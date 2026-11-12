import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { restartAgent, restartFresh } from "../lib/agent-actions";
import { useStore } from "../store";
import { TerminalHost } from "./TerminalHost";

const show = vi.fn();
const hideAll = vi.fn();
const has = vi.fn();
const sessionManagerCtor = vi.fn();

vi.mock("../lib/agent-actions", () => ({
  restartAgent: vi.fn(),
  restartFresh: vi.fn(),
}));

vi.mock("../session", () => ({
  SessionManager: vi.fn().mockImplementation(function SessionManager(
    ...args: unknown[]
  ) {
    sessionManagerCtor(...args);
    return { show, hideAll, has };
  }),
}));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  show.mockReset();
  hideAll.mockReset();
  has.mockReset();
  sessionManagerCtor.mockReset();
});

describe("TerminalHost", () => {
  function session(id: string, wait = false) {
    return {
      id,
      project: "corral",
      feature: id.split("/")[1],
      running: true,
      exit_code: null,
      killed: false,
      waiting: wait,
      last_summary: null,
    };
  }

  it("renders the #terminals container and constructs SessionManager once", () => {
    const { container } = render(<TerminalHost />);
    expect(container.querySelector("#terminals")).not.toBeNull();
    expect(useStore.getState().sessionMgr).not.toBeNull();
  });

  it("does not re-show the terminal when a session's waiting status flips", () => {
    has.mockReturnValue(true);
    useStore.setState({
      selectedId: "corral/hulling",
      sessions: new Map([["corral/hulling", session("corral/hulling")]]),
    });
    render(<TerminalHost />);
    show.mockClear();

    // A status event for the selected session allocates a fresh Map; only
    // `running` is derived, so this must not re-run `show()` (and its
    // focus grab).
    useStore.setState((s) => ({
      sessions: new Map(s.sessions).set(
        "corral/hulling",
        session("corral/hulling", true),
      ),
    }));
    expect(show).not.toHaveBeenCalled();

    // Same for a backgrounded worktree's status event.
    useStore.setState((s) => ({
      sessions: new Map(s.sessions).set(
        "corral/other",
        session("corral/other", true),
      ),
    }));
    expect(show).not.toHaveBeenCalled();
  });

  it("shows the terminal for a selected worktree with an attached session", () => {
    has.mockReturnValue(true);
    useStore.setState({ selectedId: "corral/hulling" });
    render(<TerminalHost />);
    expect(show).toHaveBeenCalledWith("corral/hulling");
  });

  it("hides all terminals when nothing is selected", () => {
    render(<TerminalHost />);
    expect(hideAll).toHaveBeenCalled();
  });

  it("re-shows the terminal when returning to the agent tab", () => {
    has.mockReturnValue(true);
    useStore.setState({ selectedId: "corral/hulling", activeTab: "diff" });
    const { rerender } = render(<TerminalHost />);
    show.mockClear();

    useStore.setState({ activeTab: "agent" });
    rerender(<TerminalHost />);

    expect(show).toHaveBeenCalledWith("corral/hulling");
  });

  it("re-shows the terminal when Settings closes", () => {
    has.mockReturnValue(true);
    useStore.setState({ selectedId: "corral/hulling", settingsOpen: true });
    const { rerender } = render(<TerminalHost />);
    show.mockClear();

    useStore.setState({ settingsOpen: false });
    rerender(<TerminalHost />);

    expect(show).toHaveBeenCalledWith("corral/hulling");
  });

  it("wires onRestart to restartAgent", () => {
    render(<TerminalHost />);
    expect(sessionManagerCtor).toHaveBeenCalledWith(
      expect.objectContaining({ onRestart: restartAgent }),
    );
  });

  it("wires onResumeFailed to restartFresh", () => {
    render(<TerminalHost />);
    expect(sessionManagerCtor).toHaveBeenCalledWith(
      expect.objectContaining({ onResumeFailed: restartFresh }),
    );
  });
});
