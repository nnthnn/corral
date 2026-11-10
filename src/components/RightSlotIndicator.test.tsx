import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RightSlotIndicator } from "./RightSlotIndicator";

describe("RightSlotIndicator", () => {
  it("renders nothing for kind 'none'", () => {
    const { container } = render(
      <RightSlotIndicator indicator={{ kind: "none" }} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders a titled spinner for kind 'running'", () => {
    const { getByTitle } = render(
      <RightSlotIndicator indicator={{ kind: "running" }} />,
    );
    expect(getByTitle("Run command active")).toBeInTheDocument();
  });

  it("renders the attention bell for kind 'attention'", () => {
    const { getByTitle } = render(
      <RightSlotIndicator indicator={{ kind: "attention" }} />,
    );
    expect(getByTitle("Waiting for input")).toHaveTextContent("🔔");
  });

  it("renders a conflict warning for kind 'conflict'", () => {
    const { getByTitle } = render(
      <RightSlotIndicator indicator={{ kind: "conflict" }} />,
    );
    expect(
      getByTitle("Would conflict with the base branch"),
    ).toBeInTheDocument();
  });

  it("renders a danger icon for kind 'checks-failed'", () => {
    const { getByTitle, container } = render(
      <RightSlotIndicator indicator={{ kind: "checks-failed" }} />,
    );
    expect(getByTitle("Pull request checks failed")).toBeInTheDocument();
    expect(container.querySelector("svg")).toHaveClass("text-danger");
  });

  it("renders a spinner for kind 'checks-running'", () => {
    const { getByTitle } = render(
      <RightSlotIndicator indicator={{ kind: "checks-running" }} />,
    );
    expect(getByTitle("Pull request checks running")).toBeInTheDocument();
  });

  it("renders both stat badges for kind 'diff' when both are non-zero", () => {
    const { getByText } = render(
      <RightSlotIndicator indicator={{ kind: "diff", added: 5, removed: 2 }} />,
    );
    expect(getByText("+5")).toBeInTheDocument();
    expect(getByText("-2")).toBeInTheDocument();
  });

  it("omits a zero diff side for kind 'diff'", () => {
    const { getByText, queryByText } = render(
      <RightSlotIndicator indicator={{ kind: "diff", added: 5, removed: 0 }} />,
    );
    expect(getByText("+5")).toBeInTheDocument();
    expect(queryByText("-0")).not.toBeInTheDocument();
  });

  it("renders both ahead/behind badges for kind 'sync' when both are non-zero", () => {
    const { getByText } = render(
      <RightSlotIndicator indicator={{ kind: "sync", ahead: 3, behind: 1 }} />,
    );
    expect(getByText("↑3")).toBeInTheDocument();
    expect(getByText("↓1")).toBeInTheDocument();
  });

  it("omits a zero sync side for kind 'sync'", () => {
    const { getByText, queryByText } = render(
      <RightSlotIndicator indicator={{ kind: "sync", ahead: 3, behind: 0 }} />,
    );
    expect(getByText("↑3")).toBeInTheDocument();
    expect(queryByText("↓0")).not.toBeInTheDocument();
  });
});
