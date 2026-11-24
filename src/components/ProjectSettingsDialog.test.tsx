import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import { ProjectSettingsDialog } from "./ProjectSettingsDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
});

describe("ProjectSettingsDialog", () => {
  it("loads and displays the project's current globs, setup, and run command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      copy_globs: ["CLAUDE.md", "IDEAS.md"],
      setup_command: "pnpm install",
      auto_setup: true,
      run_command: "pnpm dev",
      project_prompt: "use pnpm, not npm",
      env: ["API_KEY=abc", "DEBUG=true"],
    });
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    const globsTextarea = await screen.findByRole("textbox", {
      name: /Files to copy/,
    });
    expect(globsTextarea).toHaveValue("CLAUDE.md\nIDEAS.md");
    expect(screen.getByDisplayValue("pnpm install")).toBeInTheDocument();
    expect(screen.getByDisplayValue("pnpm dev")).toBeInTheDocument();
    expect(screen.getByDisplayValue("use pnpm, not npm")).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: /Environment variables/ }),
    ).toHaveValue("API_KEY=abc\nDEBUG=true");
  });

  it("saves the edited globs and setup command", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        copy_globs: ["CLAUDE.md"],
        setup_command: "",
        auto_setup: true,
        run_command: "",
        project_prompt: "",
        env: [],
      })
      .mockResolvedValueOnce(undefined);
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    const globsTextarea = await screen.findByDisplayValue("CLAUDE.md");
    fireEvent.change(globsTextarea, {
      target: { value: "docs/*.md\n\nCLAUDE.md" },
    });
    fireEvent.change(screen.getByPlaceholderText(/pnpm install/), {
      target: { value: "pnpm install && cargo fetch" },
    });
    fireEvent.click(screen.getByText("Save"));

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_project_settings", {
        project: "corral",
        settings: {
          copy_globs: ["docs/*.md", "CLAUDE.md"],
          setup_command: "pnpm install && cargo fetch",
          auto_setup: true,
          run_command: "",
          project_prompt: "",
          env: [],
        },
      }),
    );
    expect(useStore.getState().projectSettingsTarget).toBeNull();
  });

  it("saves an edited run command", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        copy_globs: [],
        setup_command: "",
        auto_setup: true,
        run_command: "",
        project_prompt: "",
        env: [],
      })
      .mockResolvedValueOnce(undefined);
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    await screen.findByText("Save");
    fireEvent.change(screen.getByPlaceholderText(/pnpm dev/), {
      target: { value: "pnpm dev" },
    });
    fireEvent.click(screen.getByText("Save"));

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_project_settings", {
        project: "corral",
        settings: {
          copy_globs: [],
          setup_command: "",
          auto_setup: true,
          run_command: "pnpm dev",
          project_prompt: "",
          env: [],
        },
      }),
    );
  });

  it("saves an edited agent intro prompt", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        copy_globs: [],
        setup_command: "",
        auto_setup: true,
        run_command: "",
        project_prompt: "",
        env: [],
      })
      .mockResolvedValueOnce(undefined);
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    await screen.findByText("Save");
    fireEvent.change(screen.getByPlaceholderText(/project-specific context/), {
      target: { value: "This repo uses pnpm, not npm." },
    });
    fireEvent.click(screen.getByText("Save"));

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_project_settings", {
        project: "corral",
        settings: {
          copy_globs: [],
          setup_command: "",
          auto_setup: true,
          run_command: "",
          project_prompt: "This repo uses pnpm, not npm.",
          env: [],
        },
      }),
    );
  });

  it("saves edited environment variables as KEY=VALUE lines", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        copy_globs: [],
        setup_command: "",
        auto_setup: true,
        run_command: "",
        project_prompt: "",
        env: [],
      })
      .mockResolvedValueOnce(undefined);
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    await screen.findByText("Save");
    fireEvent.change(screen.getByPlaceholderText(/API_KEY/), {
      target: { value: "API_KEY=abc\n\nFEATURE_FLAGS=beta" },
    });
    fireEvent.click(screen.getByText("Save"));

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_project_settings", {
        project: "corral",
        settings: {
          copy_globs: [],
          setup_command: "",
          auto_setup: true,
          run_command: "",
          project_prompt: "",
          env: ["API_KEY=abc", "FEATURE_FLAGS=beta"],
        },
      }),
    );
  });

  it("shows auto-setup unchecked when the project has it turned off", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      copy_globs: [],
      setup_command: "pnpm install",
      auto_setup: false,
      run_command: "",
      project_prompt: "",
      env: [],
    });
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    const checkbox = await screen.findByRole("checkbox", {
      name: /Run the setup command automatically/,
    });
    expect(checkbox).not.toBeChecked();
  });

  it("treats a project saved before auto_setup existed as checked", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      copy_globs: [],
      setup_command: "pnpm install",
      // An older backend never sends this field at all.
      run_command: "",
      project_prompt: "",
      env: [],
    });
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    const checkbox = await screen.findByRole("checkbox", {
      name: /Run the setup command automatically/,
    });
    expect(checkbox).toBeChecked();
  });

  it("saves auto-setup turned off when the box is unchecked", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        copy_globs: [],
        setup_command: "pnpm install",
        auto_setup: true,
        run_command: "",
        project_prompt: "",
        env: [],
      })
      .mockResolvedValueOnce(undefined);
    useStore.setState({ projectSettingsTarget: "corral" });
    render(<ProjectSettingsDialog />);

    fireEvent.click(
      await screen.findByRole("checkbox", {
        name: /Run the setup command automatically/,
      }),
    );
    fireEvent.click(screen.getByText("Save"));

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_project_settings", {
        project: "corral",
        settings: {
          copy_globs: [],
          setup_command: "pnpm install",
          auto_setup: false,
          run_command: "",
          project_prompt: "",
          env: [],
        },
      }),
    );
  });
});
