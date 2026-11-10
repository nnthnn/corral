# Corral — Theme, Voice & Mascot

## The Mascot: Buck

Buck is Corral's mascot — an angry brawler cowboy who exists for one reason:
to whip your agents into shape. He's not mean, he's *focused*. He has the
energy of a ranch hand who's seen too many agents go rogue and has zero
patience for nonsense. He's competent, direct, and permanently slightly
pissed off — not at *you*, at the chaos. You're the one he's fighting for.

Buck is a professional. He doesn't panic. He doesn't celebrate. He just
handles it and moves on to the next problem.

**Visual character:**
- Square cubic head, neck as wide as his head
- Massive shoulders, short stocky brawler proportions
- Dark charcoal beat-up cowboy hat
- Red bandana around neck
- Worn light denim work shirt
- Permanently furrowed brow, clenched fists
- Cel shaded, 90s arcade brawler style (Final Fight / Streets of Rage)

**Assets:** `src/assets/buck/`
- `buck-icon.png` — angry face close-up, app icon
- `buck-pointing.png` — full body, pointing at viewer, hero/onboarding
- `buck-sheet.png` — canonical character sheet, front/side/back reference
- *(add poses as generated)*

---

## Voice & Tone

Buck speaks in short, declarative sentences. No fluff. No apologies. No
exclamation points — Buck doesn't get excited, he gets things done.

**The rules:**
- Terse and direct. One sentence where possible.
- Active voice always. Things don't "fail" — Buck stops them.
- Slightly threatening undertone, but always on your side.
- No corporate softening ("We're having trouble..." → no. Just say what happened.)
- No em-dashes in copy. Buck doesn't pause for effect — he just says it.
- Cowboy flavor is *light*. One word or phrase per message max. Don't overdo it.
- Never cutesy. Buck tolerates no cutesy.

**Vocabulary Buck uses:**
- Herd, corral, round up, wrangle, rein in, brand, stampede, spook, drift
- Steady, handled, done, stopped, watching, waiting
- Rogue, loose, spooked (for misbehaving agents)

**Vocabulary Buck never uses:**
- Amazing, awesome, great job
- Oops, whoops, uh oh
- Please, sorry (Buck doesn't apologize — he fixes)
- We, our (Buck speaks plainly, not corporately)

---

## Copy by State

### Agent States

| State | Status Label | Buck's Internal Monologue |
|---|---|---|
| No agent | `no agent` | Waiting on you. |
| Idle | `idle` | Standing by. |
| Working | `working` | On it. |
| Waiting for input | `waiting` | Needs you. |
| Stopped (clean) | `done` | Handled. |
| Stopped (crash) | `stopped` | Went down. |
| Error / conflict | `conflict` | Gone rogue. |

### Notification / Toast Copy

**Agent launched:**
> Turned loose. Buck's watching.

**Agent completed successfully:**
> Done. Moving on.

**Agent stopped cleanly:**
> Reined in.

**Agent crashed:**
> Went down hard. Check the logs.

**Agent waiting for input:**
> This one needs you. Don't leave it standing there.

**Agent conflict (git):**
> Two of them hit the same ground. Sort it out.

**Worktree created:**
> New pen's ready.

**Worktree removed:**
> Cleared out.

**Multiple agents running:**
> Herd's moving.

**All agents idle:**
> Corral's quiet. Too quiet.

**All agents done:**
> All handled. Good run.

### Empty States

**No worktrees:**
> Nothing's running. Start something.

**No agents in worktree:**
> No agent in this pen. Launch one.

**No changed files:**
> Clean. Nothing to show.

**No projects:**
> Empty corral. Add a project to get started.

### Error States

**Failed to create worktree:**
> Couldn't build the pen. Check the path and try again.

**Failed to launch agent:**
> Wouldn't start. Check your claude PATH.

**Git operation failed:**
> Git pushed back. See what's blocking it.

**Tauri/backend error:**
> Something broke on my end. Restart and try again.

### Onboarding / First Run

**Welcome screen:**
> Buck runs this corral. You bring the work, he'll keep the agents in line.

**First worktree prompt:**
> Pick a project. Buck'll set up the pen.

**First agent launch:**
> Turn it loose. Buck's got eyes on it.

---

## Mascot Usage Guidelines

Buck appears in **three contexts only** — don't dilute him:

1. **App icon** — the angry face close-up. Always.
2. **Empty states** — full body or face, when there's nothing to show yet.
   Use the pointing pose for onboarding/first-run, the angry face for
   error states.
3. **Notifications/toasts** — text only, no image. Buck's voice, not his face.

**Buck does NOT appear:**
- In the main chrome (sidebar, panels, tabs) — DESIGN.md is explicit: cowboy
  flavor stays out of chrome.
- On every success — Buck doesn't throw a party. He moves on.
- Inline in the terminal pane — that's sacred developer space.
- As a loading spinner or decorative element — Buck is not wallpaper.

**Buck's expression never changes.** He's always slightly angry. There is no
"happy Buck." There is "Buck handled it" and "Buck is handling it" — both
look the same. That's the joke and the brand.

---

## Relationship to DESIGN.md

THEME.md (voice, mascot, copy) and DESIGN.md (visual system, tokens,
components) are intentionally separate. A new pose doesn't touch DESIGN.md.
A new component doesn't touch THEME.md. The one overlap is the accent stripe
color (`primary: #c4483a`) — Buck's bandana red — which is the one place the
cowboy aesthetic bleeds into the visual system, intentionally and exclusively.

When writing new copy: consult THEME.md.
When building new UI: consult DESIGN.md.
When in doubt about tone: ask what Buck would say. Then remove half the words.
