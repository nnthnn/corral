import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import "@testing-library/jest-dom/vitest";

// Without Vitest's `globals: true` mode, @testing-library/react can't detect
// a global `afterEach` to register its own automatic cleanup, so each
// render() would leak into the next test's DOM otherwise.
afterEach(() => {
  cleanup();
});

// jsdom (as of v29) reflects <dialog>'s `open` attribute/property but doesn't
// implement showModal()/close() — every dialog component (DeleteDialog,
// RenameDialog, RemoveProjectDialog) depends on both, so polyfill minimally
// here rather than in each test file. Guarded so a future jsdom that does
// implement these isn't silently overridden.
if (!HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
}
if (!HTMLDialogElement.prototype.close) {
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.removeAttribute("open");
    this.dispatchEvent(new Event("close"));
  };
}

// jsdom has no ResizeObserver — RunPanel's self-healing xterm
// resize effects (see their module docs) need one to exist, even in tests
// that never trigger an actual resize.
if (!("ResizeObserver" in globalThis)) {
  class MockResizeObserver {
    observe() {}
    disconnect() {}
  }
  // biome-ignore lint/suspicious/noExplicitAny: minimal test polyfill, not a full ResizeObserver
  (globalThis as any).ResizeObserver = MockResizeObserver;
}

// jsdom has no scrollIntoView implementation — the sidebar's worktree-nudge
// effect (WorktreeTree.tsx) calls it to bring a project's "+" button into
// view, which would otherwise throw in every test that triggers the effect.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
