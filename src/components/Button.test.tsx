import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "./Button";

describe("Button", () => {
  it("renders the neutral outlined default with no mode/color class", () => {
    render(<Button>Go</Button>);
    const btn = screen.getByRole("button", { name: "Go" });
    expect(btn.className.split(" ")).toEqual(["btn"]);
  });

  it("renders primary as filled", () => {
    render(<Button variant="primary">Go</Button>);
    const btn = screen.getByRole("button", { name: "Go" });
    expect(btn.className.split(" ")).toContain("primary");
  });

  it("renders danger-outlined with both danger and outlined classes", () => {
    render(<Button variant="danger-outlined">Stop</Button>);
    const btn = screen.getByRole("button", { name: "Stop" });
    expect(btn.className.split(" ")).toEqual(
      expect.arrayContaining(["danger-outlined"]),
    );
  });

  it("renders danger as filled danger", () => {
    render(<Button variant="danger">Delete</Button>);
    const btn = screen.getByRole("button", { name: "Delete" });
    expect(btn.className.split(" ")).toContain("danger");
    expect(btn.className.split(" ")).not.toContain("outlined");
  });

  it("renders ghost with no color class", () => {
    render(<Button variant="ghost">X</Button>);
    const btn = screen.getByRole("button", { name: "X" });
    expect(btn.className.split(" ")).toEqual(["btn", "ghost"]);
  });

  it("renders ghost-danger with both ghost and danger classes", () => {
    render(<Button variant="ghost-danger">Remove</Button>);
    const btn = screen.getByRole("button", { name: "Remove" });
    expect(btn.className.split(" ")).toEqual(
      expect.arrayContaining(["ghost", "danger"]),
    );
  });

  it("combines icon/square/active with any variant", () => {
    render(
      <Button variant="ghost" icon square active>
        #
      </Button>,
    );
    const btn = screen.getByRole("button", { name: "#" });
    const classes = btn.className.split(" ");
    expect(classes).toEqual(
      expect.arrayContaining(["ghost", "icon", "icon-square", "active"]),
    );
  });
});
