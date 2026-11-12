import { describe, expect, it } from "vitest";
import { appendBoundedOutput, drainOutput } from "./output-buffer";

describe("appendBoundedOutput", () => {
  it("appends without trimming below the cap", () => {
    expect(appendBoundedOutput(undefined, [1, 2], 10)).toEqual({
      output: [1, 2],
      dropped: 0,
    });
    expect(
      appendBoundedOutput({ output: [1, 2], dropped: 0 }, [3], 10),
    ).toEqual({ output: [1, 2, 3], dropped: 0 });
  });

  it("trims the oldest bytes and records how many were dropped", () => {
    const first = appendBoundedOutput(undefined, [1, 2, 3, 4], 4);
    const second = appendBoundedOutput(first, [5, 6], 4);
    expect(second).toEqual({ output: [3, 4, 5, 6], dropped: 2 });
  });
});

describe("drainOutput", () => {
  it("returns everything after the cursor", () => {
    const state = { output: [1, 2, 3, 4], dropped: 0 };
    expect(drainOutput(state, 0)).toEqual({
      data: [1, 2, 3, 4],
      next: 4,
      reset: false,
    });
    expect(drainOutput(state, 2)).toEqual({
      data: [3, 4],
      next: 4,
      reset: false,
    });
  });

  it("returns nothing once the cursor has caught up", () => {
    expect(drainOutput({ output: [1, 2], dropped: 0 }, 2)).toEqual({
      data: [],
      next: 2,
      reset: false,
    });
  });

  it("returns only new bytes after the buffer front-trimmed at its cap", () => {
    // Capped at 4: [1,2,3,4] was fully drained, then 5 and 6 arrived and
    // pushed 1 and 2 off the front. The length is unchanged (4), which is
    // exactly why a length-based cursor froze the terminal here.
    const state = { output: [3, 4, 5, 6], dropped: 2 };
    expect(drainOutput(state, 4)).toEqual({
      data: [5, 6],
      next: 6,
      reset: false,
    });
  });

  it("skips bytes trimmed before the cursor could catch up", () => {
    // The renderer last saw byte 1, but five bytes were trimmed since; the
    // oldest retained byte is all it can be shown.
    const state = { output: [6, 7, 8, 9], dropped: 5 };
    expect(drainOutput(state, 1)).toEqual({
      data: [6, 7, 8, 9],
      next: 9,
      reset: false,
    });
  });

  it("reports a reset when the stream rewinds for a fresh run", () => {
    // A fresh run reset the buffer; the cursor still points at the old run's
    // end, so the caller must clear before replaying.
    expect(drainOutput({ output: [], dropped: 0 }, 500)).toEqual({
      data: [],
      next: 0,
      reset: true,
    });
    expect(drainOutput({ output: [7, 8], dropped: 0 }, 500)).toEqual({
      data: [7, 8],
      next: 2,
      reset: true,
    });
  });
});
