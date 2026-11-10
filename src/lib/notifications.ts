import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { useStore } from "../store";
import type { SoundChoice } from "../types";
import { worktreeTitle } from "../utils";
import { invoke } from "./tauri";

export async function initNotifications(): Promise<void> {
  // Best-effort: trigger the permission prompt on platforms that have one.
  // We don't gate sends on the result — Linux desktop reports "not granted"
  // even though the DBus notification works.
  try {
    if (!(await isPermissionGranted())) await requestPermission();
  } catch {
    /* ignore */
  }
  try {
    const icon = (await invoke("notify_icon")) ?? undefined;
    useStore.setState({ notifyIcon: icon });
  } catch {
    /* icon optional */
  }
}

export function notify(title: string, body: string): void {
  if (!useStore.getState().settings.notifications_enabled) return;
  try {
    void invoke("send_notification", {
      title,
      body,
      icon: useStore.getState().notifyIcon ?? null,
    });
  } catch {
    /* notifications unavailable */
  }
}

// Lazily-created and reused across calls: browsers cap the number of live
// AudioContext instances per page (Chromium: ~6), so a long-running session
// with many "agent is waiting" notifications must not create a new one
// every time.
let audioContext: AudioContext | null = null;

/** Test-only: clears the cached AudioContext so tests can stub a fresh one. */
export function resetAudioContextForTests(): void {
  audioContext = null;
}

function playChime(ctx: AudioContext): void {
  const now = ctx.currentTime;
  for (const [freq, start] of [
    [880, 0],
    [1320, 0.09],
  ] as const) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.frequency.value = freq;
    osc.connect(gain);
    gain.connect(ctx.destination);
    gain.gain.setValueAtTime(0.0001, now + start);
    gain.gain.exponentialRampToValueAtTime(0.2, now + start + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, now + start + 0.15);
    osc.start(now + start);
    osc.stop(now + start + 0.16);
  }
}

function playWhistle(ctx: AudioContext): void {
  const now = ctx.currentTime;
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.frequency.setValueAtTime(600, now);
  osc.frequency.linearRampToValueAtTime(950, now + 0.12);
  osc.frequency.linearRampToValueAtTime(700, now + 0.3);
  osc.connect(gain);
  gain.connect(ctx.destination);
  gain.gain.setValueAtTime(0.0001, now);
  gain.gain.exponentialRampToValueAtTime(0.18, now + 0.02);
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.34);
  osc.start(now);
  osc.stop(now + 0.36);
}

function playHoller(ctx: AudioContext): void {
  const now = ctx.currentTime;
  // Two quick rising "yip"s — a triangle wave carries more harmonic
  // content than the sine used by chime/whistle, so it stays audible on
  // small/tinny laptop speakers despite the low base frequency (a plain
  // sine down at 220-440Hz was previously landing below what most laptop
  // speakers reproduce at a perceptible volume).
  for (const [start, from, to] of [
    [0, 330, 660],
    [0.16, 260, 520],
  ] as const) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "triangle";
    osc.frequency.setValueAtTime(from, now + start);
    osc.frequency.exponentialRampToValueAtTime(to, now + start + 0.1);
    osc.connect(gain);
    gain.connect(ctx.destination);
    gain.gain.setValueAtTime(0.0001, now + start);
    gain.gain.exponentialRampToValueAtTime(0.28, now + start + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, now + start + 0.16);
    osc.start(now + start);
    osc.stop(now + start + 0.18);
  }
}

function playMoo(ctx: AudioContext): void {
  const now = ctx.currentTime;
  const osc = ctx.createOscillator();
  osc.type = "sawtooth";
  osc.frequency.setValueAtTime(110, now);
  osc.frequency.linearRampToValueAtTime(135, now + 0.15);
  osc.frequency.linearRampToValueAtTime(90, now + 0.75);

  // Slow pitch wobble — a beeline sine, not a synthesized moo, without it.
  const lfo = ctx.createOscillator();
  lfo.frequency.value = 6;
  const lfoGain = ctx.createGain();
  lfoGain.gain.value = 6;
  lfo.connect(lfoGain);
  lfoGain.connect(osc.frequency);

  // Bandpass formant gives the raw sawtooth its nasal "moo" timbre instead
  // of a flat buzz.
  const filter = ctx.createBiquadFilter();
  filter.type = "bandpass";
  filter.frequency.value = 600;
  filter.Q.value = 4;

  const gain = ctx.createGain();
  gain.gain.setValueAtTime(0.0001, now);
  gain.gain.exponentialRampToValueAtTime(0.3, now + 0.08);
  gain.gain.setValueAtTime(0.3, now + 0.4);
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.8);

  osc.connect(filter);
  filter.connect(gain);
  gain.connect(ctx.destination);

  osc.start(now);
  lfo.start(now);
  osc.stop(now + 0.85);
  lfo.stop(now + 0.85);
}

const SOUND_PLAYERS: Record<SoundChoice, (ctx: AudioContext) => void> = {
  chime: playChime,
  whistle: playWhistle,
  holler: playHoller,
  moo: playMoo,
};

/** Options for the sound-choice dropdown in Settings, in display order. */
export const SOUND_OPTIONS: { id: SoundChoice; label: string }[] = [
  { id: "chime", label: "Chime" },
  { id: "whistle", label: "Whistle" },
  { id: "holler", label: "Holler" },
  { id: "moo", label: "Moo" },
];

/**
 * Play one of `SOUND_PLAYERS` via the Web Audio API — no bundled audio
 * asset needed. Bypasses the `sound_enabled` setting, so it also serves as
 * the Settings "Test" button's preview.
 */
export function playSoundChoice(choice: SoundChoice): void {
  try {
    audioContext ??= new AudioContext();
    (SOUND_PLAYERS[choice] ?? playChime)(audioContext);
  } catch {
    /* Web Audio unavailable (e.g. headless test env) */
  }
}

/** Independent of `notifications_enabled`: can be on with or without the OS
 * popup. Gated on `sound_enabled` — use `playSoundChoice` directly to
 * preview a sound regardless of that setting. */
export function playSound(): void {
  const settings = useStore.getState().settings;
  if (!settings.sound_enabled) return;
  playSoundChoice(settings.sound_choice);
}

const WAITING_HOLLERS = [
  "This one needs you. Don't leave it standing there.",
  "Needs you. Waiting at the gate.",
  "One's waiting on you. Don't let it drift.",
  "Standing at the prompt. Go see to it.",
  "It's waiting. Get over there.",
  "Needs your hand. Won't move without it.",
  "One's idle, waiting on you.",
];

function holler(): string {
  return WAITING_HOLLERS[Math.floor(Math.random() * WAITING_HOLLERS.length)];
}

/** The worktree's display name — its renamed title when one is known (that's
 * what the sidebar shows), falling back to the feature slug for a worktree
 * that hasn't been renamed or has already left the store by the time the
 * notification fires. The project is used only to find the worktree; it isn't
 * shown, since the app icon already identifies Corral. */
function notificationSubject(project: string, feature: string): string {
  const wt = useStore
    .getState()
    .worktrees.find((w) => w.project === project && w.feature === feature);
  return wt ? worktreeTitle(wt) : feature;
}

/** Notify when a backgrounded agent goes quiet at the prompt (once per turn). */
export function maybeNotifyWaiting(id: string): void {
  const s = useStore.getState();
  const sess = s.sessions.get(id);
  if (!sess?.running) return;
  // Only nudge if you're not already looking at it.
  if (document.hasFocus() && id === s.selectedId) return;
  // Collapse the trailing-render burst into a single ping per turn.
  if (s.notifiedWaiting.has(id)) return;
  const notifiedWaiting = new Set(s.notifiedWaiting);
  notifiedWaiting.add(id);
  useStore.setState({ notifiedWaiting });
  notify(holler(), notificationSubject(sess.project, sess.feature));
  playSound();
}

/** Notify when a backgrounded agent's process exits — cleanly, by crash, or
 * by a deliberate stop. Mirrors `maybeNotifyWaiting`'s "don't nag if you're
 * already looking at it" suppression. */
export function notifyExit(
  id: string,
  project: string,
  feature: string,
  code: number | null,
  killed: boolean,
): void {
  const s = useStore.getState();
  if (document.hasFocus() && id === s.selectedId) return;
  const title = killed
    ? "Reined in."
    : code === 0
      ? "Done. Moving on."
      : "Went down hard. Check the logs.";
  notify(title, notificationSubject(project, feature));
  playSound();
}
