import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "../store";
import { FilePreview } from "./FilePreview";

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
});

describe("FilePreview", () => {
  it("shows a loading indicator while fileContent is null", () => {
    useStore.setState({ fileTab: { path: "src/main.rs" } });
    render(<FilePreview />);
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("renders text content line by line for an unknown language", () => {
    useStore.setState({
      fileTab: { path: "notes.txt" },
      fileContent: { kind: "text", text: "line one\nline two" },
    });
    render(<FilePreview />);
    expect(screen.getByText("line one")).toBeInTheDocument();
    expect(screen.getByText("line two")).toBeInTheDocument();
  });

  it("syntax-highlights a known-language text file", () => {
    useStore.setState({
      fileTab: { path: "src/main.rs" },
      fileContent: { kind: "text", text: "fn main() {}" },
    });
    const { container } = render(<FilePreview />);
    // "fn" is a Rust keyword, so it should be wrapped in a hljs token span.
    expect(container.querySelector(".hljs-keyword")?.textContent).toBe("fn");
  });

  it("shows a binary-file message", () => {
    useStore.setState({
      fileTab: { path: "logo.png" },
      fileContent: { kind: "binary" },
    });
    render(<FilePreview />);
    expect(screen.getByText("Binary file, not previewed.")).toBeInTheDocument();
  });

  it("shows a too-large message with a formatted size", () => {
    useStore.setState({
      fileTab: { path: "big.bin" },
      fileContent: { kind: "too_large", size: 2 * 1024 * 1024 },
    });
    render(<FilePreview />);
    expect(
      screen.getByText("File too large to preview (2.0 MB)."),
    ).toBeInTheDocument();
  });

  it("shows the error line when fileError is set", () => {
    useStore.setState({
      fileTab: { path: "src/main.rs" },
      fileError: "not found",
    });
    render(<FilePreview />);
    expect(screen.getByText("not found")).toHaveClass("err");
  });

  it("renders at the configured diff_font_size", () => {
    useStore.setState({
      fileTab: { path: "notes.txt" },
      fileContent: { kind: "text", text: "line" },
      settings: { ...useStore.getState().settings, diff_font_size: 22 },
    });
    render(<FilePreview />);
    expect(screen.getByText("line").closest("pre")).toHaveStyle({
      fontSize: "22px",
    });
  });

  it("renders a raster image from its base64 data URL", () => {
    useStore.setState({
      fileTab: { path: "assets/logo.png" },
      fileContent: { kind: "image", mime: "image/png", data: "iVBORw0KGgo=" },
    });
    render(<FilePreview />);
    expect(screen.getByAltText("assets/logo.png")).toHaveAttribute(
      "src",
      "data:image/png;base64,iVBORw0KGgo=",
    );
    // No source view and no text line for a raster image.
    expect(screen.queryByRole("button", { name: "Source" })).toBeNull();
  });

  it("renders an SVG as an image by default", () => {
    useStore.setState({
      fileTab: { path: "icon.svg" },
      fileContent: { kind: "text", text: "<svg></svg>" },
    });
    render(<FilePreview />);
    expect(screen.getByAltText("icon.svg")).toHaveAttribute(
      "src",
      `data:image/svg+xml;charset=utf-8,${encodeURIComponent("<svg></svg>")}`,
    );
  });

  it("switches an SVG between rendered preview and highlighted source", () => {
    useStore.setState({
      fileTab: { path: "icon.svg" },
      fileContent: { kind: "text", text: "<svg>\n</svg>" },
    });
    const { container } = render(<FilePreview />);

    fireEvent.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.queryByAltText("icon.svg")).toBeNull();
    // `.svg` maps to XML, so the source is tokenized, not plain.
    expect(container.querySelector(".hljs-tag")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(screen.getByAltText("icon.svg")).toBeInTheDocument();
  });

  it("resets an SVG to the rendered preview when a different file is opened", () => {
    useStore.setState({
      fileTab: { path: "a.svg" },
      fileContent: { kind: "text", text: "<svg></svg>" },
    });
    render(<FilePreview />);
    fireEvent.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.queryByAltText("a.svg")).toBeNull();

    act(() => {
      useStore.setState({
        fileTab: { path: "b.svg" },
        fileContent: { kind: "text", text: "<svg></svg>" },
      });
    });
    expect(screen.getByAltText("b.svg")).toBeInTheDocument();
  });

  it("renders markdown files as a preview by default", () => {
    useStore.setState({
      fileTab: { path: "README.md" },
      fileContent: { kind: "text", text: "# Hello\n\nsome **bold** text" },
    });
    render(<FilePreview />);
    expect(screen.getByRole("heading", { name: "Hello" })).toBeInTheDocument();
  });

  it("switches markdown to highlighted source", () => {
    useStore.setState({
      fileTab: { path: "README.md" },
      fileContent: { kind: "text", text: "# Hello" },
    });
    const { container } = render(<FilePreview />);
    expect(screen.getByRole("heading", { name: "Hello" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.queryByRole("heading", { name: "Hello" })).toBeNull();
    expect(container.querySelector("pre.diff-body")).not.toBeNull();
  });

  it("offers no view toggle for a plain text file", () => {
    useStore.setState({
      fileTab: { path: "notes.txt" },
      fileContent: { kind: "text", text: "hello" },
    });
    render(<FilePreview />);
    expect(screen.queryByRole("button", { name: "Source" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
  });
});
