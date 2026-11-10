import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHoverIntent } from "./useHoverIntent";

function Harness() {
  const hover = useHoverIntent({ openDelayMs: 400, closeDelayMs: 150 });
  return (
    <div>
      <button
        type="button"
        onMouseEnter={hover.scheduleOpen}
        onMouseLeave={hover.scheduleClose}
        onFocus={hover.openNow}
        onBlur={hover.scheduleClose}
      >
        trigger
      </button>
      {hover.open && (
        // biome-ignore lint/a11y/noStaticElementInteractions: test harness only, mirrors the real hover-card mouseenter/mouseleave wiring
        <div
          onMouseEnter={hover.cancelClose}
          onMouseLeave={hover.scheduleClose}
        >
          card
        </div>
      )}
    </div>
  );
}

describe("useHoverIntent", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not open before the open delay elapses", () => {
    render(<Harness />);
    fireEvent.mouseEnter(screen.getByText("trigger"));
    act(() => {
      vi.advanceTimersByTime(399);
    });
    expect(screen.queryByText("card")).not.toBeInTheDocument();
  });

  it("opens after the open delay elapses", () => {
    render(<Harness />);
    fireEvent.mouseEnter(screen.getByText("trigger"));
    act(() => {
      vi.advanceTimersByTime(400);
    });
    expect(screen.getByText("card")).toBeInTheDocument();
  });

  it("opens immediately on focus, bypassing the open delay", () => {
    render(<Harness />);
    fireEvent.focus(screen.getByText("trigger"));
    expect(screen.getByText("card")).toBeInTheDocument();
  });

  it("closes after the close delay once the trigger loses hover", () => {
    render(<Harness />);
    fireEvent.mouseEnter(screen.getByText("trigger"));
    act(() => {
      vi.advanceTimersByTime(400);
    });
    fireEvent.mouseLeave(screen.getByText("trigger"));
    act(() => {
      vi.advanceTimersByTime(149);
    });
    expect(screen.getByText("card")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByText("card")).not.toBeInTheDocument();
  });

  it("cancelClose keeps it open across trigger-to-card handoff", () => {
    render(<Harness />);
    fireEvent.mouseEnter(screen.getByText("trigger"));
    act(() => {
      vi.advanceTimersByTime(400);
    });
    fireEvent.mouseLeave(screen.getByText("trigger"));
    fireEvent.mouseEnter(screen.getByText("card"));
    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(screen.getByText("card")).toBeInTheDocument();
  });
});
