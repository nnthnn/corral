import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { CommandPalette } from "./CommandPalette";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const wt = (feature: string, title: string): Worktree => ({
  project: "corral",
  feature,
  slug: feature,
  branch: `feat/${feature}`,
  path: `/path/${feature}`,
  status: null,
  has_resumable_session: false,
  note: "",
  title,
});

const input = () => screen.getByPlaceholderText("Jump to a worktree…");

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset().mockResolvedValue({ base: null, files: [] });
});

describe("CommandPalette", () => {
  it("renders nothing while closed", () => {
    render(<CommandPalette />);
    expect(
      screen.queryByPlaceholderText("Jump to a worktree…"),
    ).not.toBeInTheDocument();
  });

  it("lists worktrees in sidebar order when open", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [
        wt("hulling", "Hull the deck"),
        wt("weeding", "Weed the garden"),
      ],
    });
    render(<CommandPalette />);
    expect(screen.getByText("Hull the deck")).toBeInTheDocument();
    expect(screen.getByText("Weed the garden")).toBeInTheDocument();
    // Actions are appended after the worktrees.
    expect(screen.getByText("Settings")).toBeInTheDocument();
  });

  it("filters worktrees by query, matching project/title/branch", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [
        wt("hulling", "Hull the deck"),
        wt("weeding", "Weed the garden"),
      ],
    });
    render(<CommandPalette />);

    fireEvent.change(input(), { target: { value: "weed" } });
    // `getByRole` matches the accessible name (full text, including the
    // <mark> that `Highlight` wraps around the matched substring) — unlike
    // `getByText`, which only sees direct text nodes.
    expect(
      screen.getByRole("button", { name: /Weed the garden/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Hull the deck/ }),
    ).not.toBeInTheDocument();
  });

  it("Enter selects the highlighted worktree and closes", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [
        wt("hulling", "Hull the deck"),
        wt("weeding", "Weed the garden"),
      ],
      selectedId: null,
    });
    render(<CommandPalette />);

    fireEvent.keyDown(input(), { key: "Enter" });
    expect(useStore.getState().selectedId).toBe("corral/hulling");
    expect(useStore.getState().paletteOpen).toBe(false);
  });

  it("ArrowDown + Enter selects the second worktree", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [
        wt("hulling", "Hull the deck"),
        wt("weeding", "Weed the garden"),
      ],
      selectedId: null,
    });
    render(<CommandPalette />);

    fireEvent.keyDown(input(), { key: "ArrowDown" });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(useStore.getState().selectedId).toBe("corral/weeding");
  });

  it("clicking a worktree selects it", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [wt("hulling", "Hull the deck")],
      selectedId: null,
    });
    render(<CommandPalette />);

    fireEvent.click(screen.getByText("Hull the deck"));
    expect(useStore.getState().selectedId).toBe("corral/hulling");
    expect(useStore.getState().paletteOpen).toBe(false);
  });

  it("Escape closes the palette", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [],
    });
    render(<CommandPalette />);

    fireEvent.keyDown(input(), { key: "Escape" });
    expect(useStore.getState().paletteOpen).toBe(false);
  });

  it("runs a global action (Settings) from the list", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [],
    });
    render(<CommandPalette />);

    fireEvent.change(input(), { target: { value: "settings" } });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(useStore.getState().settingsOpen).toBe(true);
    expect(useStore.getState().paletteOpen).toBe(false);
  });

  it("shows a no-matches row for a query that hits nothing", () => {
    useStore.setState({
      paletteOpen: true,
      projects: ["corral"],
      worktrees: [],
    });
    render(<CommandPalette />);

    fireEvent.change(input(), { target: { value: "zzz" } });
    expect(screen.getByText("No matches.")).toBeInTheDocument();
  });
});
