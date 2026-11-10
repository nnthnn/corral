import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ChangeStat, SyncIcon } from "./ChangeStatusBadges";

afterEach(cleanup);

describe("ChangeStat", () => {
  it("renders both sides when both are non-zero", () => {
    const { getByText } = render(
      <ChangeStat file={{ added: 5, removed: 2 }} />,
    );
    expect(getByText("+5")).toBeInTheDocument();
    expect(getByText("-2")).toBeInTheDocument();
  });

  it("omits a zero side", () => {
    const { getByText, queryByText } = render(
      <ChangeStat file={{ added: 5, removed: 0 }} />,
    );
    expect(getByText("+5")).toBeInTheDocument();
    expect(queryByText("-0")).not.toBeInTheDocument();
  });
});

describe("SyncIcon", () => {
  it("applies the state class for each sync state", () => {
    for (const state of ["uncommitted", "committed", "pushed"] as const) {
      const { container } = render(<SyncIcon state={state} />);
      expect(container.querySelector(`.change-sync.${state}`)).not.toBeNull();
      cleanup();
    }
  });

  it("adds the local-edits dot on a dirty committed/pushed file", () => {
    const { container } = render(<SyncIcon state="pushed" dirty />);
    expect(
      container.querySelector(".change-sync.has-local-edits"),
    ).not.toBeNull();
    expect(container.querySelector(".change-sync-dot")).not.toBeNull();
  });

  it("omits the dot for a dirty 'uncommitted' file", () => {
    const { container } = render(<SyncIcon state="uncommitted" dirty />);
    expect(container.querySelector(".change-sync-dot")).toBeNull();
  });

  it("omits the dot for a clean file", () => {
    const { container } = render(<SyncIcon state="pushed" />);
    expect(container.querySelector(".change-sync-dot")).toBeNull();
  });
});
