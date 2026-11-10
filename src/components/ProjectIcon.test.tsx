import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "../store";
import { ProjectIcon } from "./ProjectIcon";

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
});

describe("ProjectIcon", () => {
  it("renders an <img> for a cached icon URL", () => {
    useStore.setState({
      projectIcons: new Map([["corral", "data:image/png;base64,AA=="]]),
    });
    render(<ProjectIcon project="corral" collapsed={false} />);
    expect(screen.getByRole("presentation")).toHaveAttribute(
      "src",
      "data:image/png;base64,AA==",
    );
  });

  it("falls back to a default glyph when no icon is cached", () => {
    render(<ProjectIcon project="corral" collapsed={false} />);
    expect(screen.queryByRole("presentation")).not.toBeInTheDocument();
  });

  it("falls back to a default glyph when the lookup found nothing", () => {
    useStore.setState({ projectIcons: new Map([["corral", null]]) });
    render(<ProjectIcon project="corral" collapsed={false} />);
    expect(screen.queryByRole("presentation")).not.toBeInTheDocument();
  });
});
