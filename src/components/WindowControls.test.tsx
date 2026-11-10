import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { closeWindow } from "../lib/window-chrome";
import { useStore } from "../store";
import { WindowControls } from "./WindowControls";

vi.mock("../lib/window-chrome", () => ({
  closeWindow: vi.fn(),
}));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(closeWindow).mockReset().mockResolvedValue(undefined);
});

describe("WindowControls", () => {
  it("renders nothing when the window already has real decorations", () => {
    useStore.setState({ needsCustomChrome: false });
    const { container } = render(<WindowControls />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders only a Close button when custom chrome is needed", () => {
    useStore.setState({ needsCustomChrome: true });
    render(<WindowControls />);
    expect(screen.getByTitle("Close Corral")).toBeInTheDocument();
    expect(screen.queryByTitle("Minimize")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Maximize")).not.toBeInTheDocument();
  });

  it("calls closeWindow when the close button is clicked", async () => {
    useStore.setState({ needsCustomChrome: true });
    render(<WindowControls />);
    await userEvent.click(screen.getByTitle("Close Corral"));
    expect(closeWindow).toHaveBeenCalled();
  });
});
