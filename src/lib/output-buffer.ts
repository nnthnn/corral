/** Capped, append-only byte stream for a long-lived process whose output is
 * mirrored into the store (the Setup tab's provisioning command, the Run
 * tab's dev server).
 *
 * `output` is trimmed from the front once it grows past the cap, so it can't
 * grow unbounded for a chatty process. But a front-trimmed array alone leaves
 * a renderer unable to tell *which* bytes it has already written — once the
 * cap is hit the array length stops changing, and a length-based cursor
 * freezes the terminal even though new output keeps arriving.
 *
 * `dropped` counts the bytes trimmed off the front so far, which makes
 * `dropped + output.length` a monotonically increasing absolute cursor. A
 * renderer keeps that cursor and asks `drainOutput` for everything after it. */
export interface BoundedOutput {
  output: number[];
  /** Bytes trimmed from the front of `output` so far. */
  dropped?: number;
}

/** Cap for the Setup tab's retained output. */
export const MAX_PROVISION_OUTPUT_BYTES = 256 * 1024;

/** Cap for the Run tab's retained output. Larger than Setup's: a dev server's
 * log is the thing you scroll back through after an error, and the backend's
 * own PTY buffer is far bigger (32 MB) — this only bounds the
 * replayed-on-reattach copy held in the store. */
export const MAX_RUN_OUTPUT_BYTES = 1024 * 1024;

/** Append a chunk, trimming the oldest bytes if the result would exceed
 * `max`. `concat` rather than spread: the retained array can be hundreds of
 * kilobytes, and spreading it element-by-element per chunk is measurably
 * slower than a single bulk copy. */
export function appendBoundedOutput(
  prev: BoundedOutput | undefined,
  data: number[],
  max: number,
): BoundedOutput {
  const output = (prev?.output ?? []).concat(data);
  const dropped = prev?.dropped ?? 0;
  const overflow = output.length - max;
  return overflow > 0
    ? { output: output.slice(overflow), dropped: dropped + overflow }
    : { output, dropped };
}

export interface OutputDelta {
  /** Bytes the renderer hasn't written yet. */
  data: number[];
  /** The cursor to store for the next call. */
  next: number;
  /** The stream rewound (a fresh run replaced the buffer), so the renderer
   * must clear its terminal before writing `data`. */
  reset: boolean;
}

/** Everything emitted after `cursor` (an absolute byte offset into the same
 * stream). Bytes trimmed before the cursor was caught up are skipped rather
 * than replayed out of order — losing the very oldest output of a run whose
 * buffer overflowed is preferable to corrupting the terminal's current lines. */
export function drainOutput(
  state: BoundedOutput | undefined,
  cursor: number,
): OutputDelta {
  const output = state?.output ?? [];
  const dropped = state?.dropped ?? 0;
  const next = dropped + output.length;
  if (next < cursor) {
    // Everything the renderer had is gone — replay the new run's buffer from
    // the start of what's retained.
    return { data: output, next, reset: true };
  }
  if (next === cursor) return { data: [], next, reset: false };
  return {
    data: output.slice(Math.max(0, cursor - dropped)),
    next,
    reset: false,
  };
}
