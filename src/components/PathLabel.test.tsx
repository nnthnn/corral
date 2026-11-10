import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PathLabel } from "./PathLabel";

describe("PathLabel", () => {
  it("renders a bare filename with no directory", () => {
    render(<PathLabel path="README.md" />);
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("splits a directory and filename for a non-renamed path", () => {
    render(<PathLabel path="src/components/Foo.tsx" />);
    expect(screen.getByText("src/components/")).toBeInTheDocument();
    expect(screen.getByText("Foo.tsx")).toBeInTheDocument();
  });

  it("shows old → new filenames for a same-directory rename, with the shared dir shown once", () => {
    render(
      <PathLabel
        path="src/components/new.tsx"
        oldPath="src/components/old.tsx"
      />,
    );
    expect(screen.getByText("old.tsx")).toBeInTheDocument();
    expect(screen.getByText("new.tsx")).toBeInTheDocument();
    expect(screen.getAllByText("src/components/")).toHaveLength(1);
  });

  it("falls back to full old/new paths when the directory itself changed", () => {
    render(
      <PathLabel
        path="src/utils/newHelpers.ts"
        oldPath="src/lib/oldutils.ts"
      />,
    );
    expect(screen.getByText("src/lib/oldutils.ts")).toBeInTheDocument();
    expect(screen.getByText("src/utils/newHelpers.ts")).toBeInTheDocument();
  });
});
