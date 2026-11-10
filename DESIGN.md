---
version: "alpha"
name: corral
description: >
  Dark, dense, developer-facing UI for a Tauri desktop app that manages
  parallel Claude Code sessions across git worktrees. Warm dark foundation
  (brown-black, not slate) with a red accent — deliberately distinct from
  the slate+amber dev tool monoculture. One rugged-cowboy signature touch
  (the accent stripe) — not broad theming.
colors:
  primary: "#c4483a"
  primary-press: "#a83b2f"
  secondary: "#c49a3a"
  bg: "#1a1510"
  surface: "#221c14"
  surface-2: "#2a2318"
  border: "#3d3328"
  text: "#ede8e0"
  muted: "#8a8070"
  success: "#8fae6d"
  warning: "#c49a3a"
  danger: "#c4483a"
  decorative: "#b285a8"
typography:
  body:
    fontFamily: "Inter, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.4
  heading:
    fontFamily: "Inter, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.2
  label:
    fontFamily: "Inter, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 500
    lineHeight: 1
  mono:
    fontFamily: "ui-monospace, Menlo, monospace"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.2
  caption:
    fontFamily: "Inter, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "0.78rem"
    fontWeight: 400
    lineHeight: 1.3
  code:
    fontFamily: "ui-monospace, Menlo, monospace"
    fontSize: "0.8rem"
    fontWeight: 400
    lineHeight: 1.2
  sub:
    fontFamily: "Inter, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "0.85rem"
    fontWeight: 400
    lineHeight: 1.3
rounded:
  sm: "3px"
  md: "5px"
spacing:
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
  2xl: "20px"
components:
  btn:
    radius: "{rounded.md}"
    borderWidth: "1px"
    paddingY: "{spacing.sm}"
    paddingX: "{spacing.lg}"
    primaryBg: "{colors.primary}"
    dangerBg: "{colors.danger}"
  statusChip:
    radius: "{rounded.sm}"
    fontSize: "{typography.label.fontSize}"
  accentStripe:
    width: "2px"
    color: "{colors.primary}"
  secondaryAccent:
    color: "{colors.secondary}"
---

## Overview

Corral is a power-user dashboard, not a marketing surface — developers scan
many parallel worktree sessions at once, so density and legibility win over
decoration. The brand personality is Buck: an angry brawler cowboy who
whips agents into shape. Capable, unflashy, quietly dangerous.

The palette is deliberately warm-dark (brown-black foundation, not the
cold slate that Linear/Zed/Claude/Cursor all share) with a single red
accent — Buck's bandana red. This is not a subtle distinction; it's the
point. Corral should feel immediately different from every other dev tool
on the dock. The warmth comes from the bg/surface stack; the red accent
does the heavy lifting for actions, focus, and the active stripe.

A secondary worn-gold (`secondary: #c49a3a`) is available for git-ahead
state and decorative use only — it echoes the original amber direction
without becoming the primary accent.

That personality shows up as one deliberate signature touch — a red accent
stripe on the thing you're actively looking at — not as broad theming.
Cowboy flavor (Buck's voice, ranch language) stays confined to notification
copy and microcopy; it does not leak into chrome, icons, or layout.

Voice, mascot usage, and copy guidelines live in THEME.md — cowboy flavor
stays out of this document intentionally.

Before adding new UI: reuse a token below before hardcoding a value, and
check the [Do's and Don'ts](#dos-and-donts) checklist.

## Colors

| Token | Value | Role |
|---|---|---|
| `primary` | `#c4483a` | Primary actions, focus rings, the accent stripe — Buck's bandana red |
| `primary-press` | `#a83b2f` | Pressed/active state of primary elements |
| `secondary` | `#c49a3a` | Git-ahead state, worn-gold decorative use only — never primary actions |
| `bg` | `#1a1510` | App background — warm near-black, not cold slate |
| `surface` | `#221c14` | Panels, sidebar, rows |
| `surface-2` | `#2a2318` | Nested/raised surfaces within a panel |
| `border` | `#3d3328` | All borders (Corral has no shadows — see Elevation) |
| `text` | `#ede8e0` | Primary text — slightly warm white, not pure |
| `muted` | `#8a8070` | Secondary text, inactive badges |
| `success` | `#8fae6d` | Idle/working/good session state |
| `warning` | `#c49a3a` | Behind/dirty git state (shares secondary token) |
| `danger` | `#c4483a` | Conflict/stopped/error state (shares primary token) |
| `decorative` | `#b285a8` | Non-semantic accents (e.g. branch name label) — never used for status |

The warm-dark foundation (`bg`/`surface`/`border`) is what separates Corral
from the slate+amber dev tool monoculture. The red primary serves double
duty as both the action color and the danger/error color — this is
intentional. In Corral, taking action and encountering an error share the
same energy: something needs attention right now. The worn-gold `secondary`
is available but restrained — it's an echo, not a co-lead.

## Typography

Inter throughout; no second display font. Four core levels, deliberately
few, extended by three narrow-purpose sizes reused consistently rather than
invented ad hoc wherever a component needs something in between:

- **`body`** (14px/400) — default text everywhere.
- **`heading`** (15px/600) — sidebar title, section headers, dialog titles.
- **`label`** (11px/500) — badges, mini stats, status chips.
- **`mono`** (11px, `ui-monospace`) — branch names and other literal
  identifiers in the chrome (not the terminal itself — xterm.js manages its
  own font settings separately). Composed as `font-mono` + `label`, not a
  single dedicated class — `font-mono` also pairs with `caption`/`code`/`sub`
  elsewhere (diff paths, error text) wherever monospace family is needed at
  a different size than 11px; those aren't the `mono` level, just the same
  font family at another size.
- **`caption`** (12.5px/400) — de-emphasized helper text (form hints,
  secondary counts).
- **`code`** (12.8px/400, mono) — inline `<code>` snippets.
- **`sub`** (13.6px/400) — secondary UI text one notch below `body` (form
  labels, tab labels, button text).

One narrow, documented exception: the full-page Settings header
(`#settings-pane h2`) uses a one-off larger size (1.6rem) for its
page-level title — the only place text exceeds `heading`. It's a single
call site, not a reusable token; don't extend it elsewhere. Before adding
any other new size, reuse one of the seven tokens above.

## Layout

Fixed 3-pane app shell: sidebar (256px) | main (flex, min 480px) | changes
(280px), collapsible via width-to-zero rather than `display: none` (a
webkit2gtk repaint quirk — see root `CLAUDE.md`). Spacing scale is small and
tight (`4px`–`20px`) because this is a scanning surface, not a reading
surface — density is a feature, not a shortcut. Don't introduce a second
spacing scale; extend this one if a gap is genuinely missing.

## Elevation & Depth

No shadows, anywhere. Depth is communicated two ways only:

1. **Borders** — every panel/card/button gets a real 1px `border` — never a
   shadow standing in for one. The one exception: `.btn` uses an inset
   `box-shadow` instead of a real `border` because on this system's
   webkit2gtk, a real `border` + `border-radius` anti-aliases the stroke's
   inner and outer edges independently, leaving a visibly muddy, jagged
   corner — the inset shadow paints in the same pass as the rounded-rect
   clip, so the curve stays clean. It's visually indistinguishable from a
   1px border and carries none of the elevation implication the "no
   shadows" rule is guarding against; don't extend this technique beyond
   `.btn` without the same rendering constraint forcing it.
2. **The accent stripe** — a `primary` (red) underline on the active tab.
   This is Corral's one elevation signature, and it's reserved for exactly
   one place: the active tab. Don't extend it to the sidebar row, the
   terminal pane, hover states, or decorative use — it means "this one,
   right now." The red here is not alarming — it's Buck's hand on your
   shoulder.

## Shapes

Two radii only:

- `rounded.md` (5px) — panels, buttons, cards.
- `rounded.sm` (3px) — badges, chips, small inline controls.

Nothing gets fully rounded (pills) or fully square. If a new component
doesn't obviously fit one of these two, it's a sign to reuse an existing
component instead of inventing a third radius.

## Components

Corral's dynamic/stateful classes are driven from TypeScript (e.g.
`` `status-chip ${st.cls}` ``), so they're implemented as a small
`@layer components` block (via `@apply`) rather than inline utilities —
inline utilities handle static one-off layout only. The living component
set:

- **`.btn`** — `rounded.md`, one shared base (shape/sizing/typography/press-feedback)
  plus two independent modifiers: **color** (neutral default / `.primary` /
  `.danger`) and **mode** (`.filled` solid background, `.outlined` — the
  neutral default — transparent background with an always-visible border, or
  `.ghost` transparent background with **no border in any state**, including
  hover — ghost buttons get a background tint + brightness bump on hover
  instead). `.btn.icon` / `.btn.icon-square` / `.btn.icon.active` layer on top
  of any color/mode combination for icon-only and toggle buttons.
- **`.status-chip.*`** — `rounded.sm`, `label` typography, background tint
  of its status color (`working`/`done` share `success`, `stopped` uses
  `danger`). `.status-chip.waiting` uses `primary` (Buck's red) rather than
  a status-specific color, since "waiting" borrows the same attention
  energy as an error/action.
- **`.tab.active`** — the accent stripe (see Elevation). `.wt-item.active`
  and the focused terminal pane are marked by a plain `surface-2`
  background/no border change only — no accent stripe.
- **`.mini.ahead/.behind/.dirty/.conflict/.stat-add/.stat-del`** — colored
  only on the active row; every other row's badges fade to `muted` (existing
  rule, keep it — it's what keeps a dense list scannable). The bare
  (non-`.mini`) `.stat-add`/`.stat-del` variant is the same add/remove-count
  color pair at full size, used in the Changes pane's per-file list rather
  than the sidebar row.
- **`FeatureStatusIcon`** — the sidebar row's git-status icon (lucide,
  see Iconography), replacing an earlier plain status dot. `success`/
  `danger`/`muted` color the icon the same way status color would've colored
  a dot.

## Do's and Don'ts

- **Do** reuse a color/spacing/radius token before hardcoding a value.
- **Do** reuse an existing component class before inventing a new one.
- **Do** keep the accent stripe to its one approved spot (the active tab).
- **Don't** add a shadow — depth comes from borders and the accent stripe
  only.
- **Don't** add a second font, a second radius scale, or a pill/fully-round
  shape.
- **Don't** let cowboy theming (emoji, ranch language) leak from
  notification copy into chrome, icons, or component names.
- **Don't** loosen density "for breathing room" — this is a scanning
  dashboard; if something feels cramped, check spacing tokens are being
  used correctly before adding new ones.

## Iconography

*(Non-standard section — kept for completeness; not part of the canonical
`design.md` sections.)*

Icons via `lucide-react` only — no other icon library, no icon font. Lucide's
default `stroke-width` is 2; `stroke-linecap`/`stroke-linejoin: round` and
`fill: none` come from lucide by default, and `stroke: currentColor` so
icons inherit their container's text color automatically. A couple of
one-off inline SVGs predate the lucide adoption (e.g. `Detail.tsx`,
`ChangesPane.tsx`) at `stroke-width: 1.5` — don't add more hand-drawn SVGs;
use a lucide icon instead, even if its stroke reads slightly heavier than
those older ones.
