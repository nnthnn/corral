import {
  Activity,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CircleDot,
  CircleX,
  GitCommitHorizontal,
  GitCommitVertical,
  GitPullRequest,
  LoaderCircle,
  OctagonAlert,
  PlayCircle,
  Radio,
  RefreshCw,
  Server,
  SquareTerminal,
  TriangleAlert,
  Zap,
} from "lucide-react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BullSpinner } from "./components/BullSpinner";
import { Button, type ButtonVariant } from "./components/Button";
import { SyncIcon } from "./components/ChangeStatusBadges";
import { FeatureStatusIcon } from "./components/FeatureStatusIcon";
import { Highlight } from "./components/Highlight";
import { LassoSpinner } from "./components/LassoSpinner";
import { PathLabel } from "./components/PathLabel";
import { ProjectIcon } from "./components/ProjectIcon";
import { PrPane } from "./components/PrPane";
import { RightSlotIndicator } from "./components/RightSlotIndicator";
import {
  BellSpinner,
  GearSpinner,
  PulseSpinner,
  RingSpinner,
  WagonWheelSpinner,
  WagonWheelSpinnerMono,
  WindmillSpinner,
  WrenchSpinner,
} from "./components/spinners";
import { useStore } from "./store";
import type { CheckRun, PrDetails, PrInfo, SyncState, Worktree } from "./types";
import type {
  FeatureStatus,
  RightSlotIndicator as RightSlotIndicatorValue,
} from "./utils";
import { wtId } from "./utils";

/** Standalone gallery of corral's pure presentational components — mounted
 * via `preview.html` instead of `index.html` so it never calls `bootstrap()`
 * (no Tauri runtime available outside the desktop shell). Run with `pnpm dev`,
 * then open http://localhost:1420/preview.html. Only add
 * components here that render from props/the zustand store defaults alone;
 * anything that needs live Tauri data or session state belongs in the real
 * app instead. */

const BUTTON_VARIANTS: ButtonVariant[] = [
  "default",
  "primary",
  "danger-outlined",
  "danger",
  "ghost",
  "ghost-danger",
];

const FEATURE_STATUSES: FeatureStatus[] = [
  "new",
  "in-progress",
  "committed",
  "pr_draft",
  "pr_open",
  "pr_closed",
  "merged",
  "broken",
];

/** One example indicator per `RightSlotIndicator.kind` — see
 * `rightSlotIndicator()` in utils.ts for how a real worktree picks one. */
const RIGHT_SLOT_INDICATORS: {
  label: string;
  indicator: RightSlotIndicatorValue;
}[] = [
  { label: "running", indicator: { kind: "running" } },
  { label: "attention", indicator: { kind: "attention" } },
  { label: "checks-failed", indicator: { kind: "checks-failed" } },
  { label: "checks-running", indicator: { kind: "checks-running" } },
  { label: "conflict", indicator: { kind: "conflict" } },
  { label: "diff", indicator: { kind: "diff", added: 42, removed: 7 } },
  { label: "sync", indicator: { kind: "sync", ahead: 3, behind: 1 } },
  { label: "none", indicator: { kind: "none" } },
];

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3 pb-6 mb-6 border-b border-border">
      <h2 className="text-sm font-semibold text-muted uppercase tracking-wide">
        {title}
      </h2>
      <div className="flex flex-wrap items-center gap-3">{children}</div>
    </section>
  );
}

/** Like `Section`, but its children lay out as a wrapping card grid instead
 * of an inline row — for galleries where each entry carries enough detail
 * (name, token, trigger condition) to need real width. */
function CardSection({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3 pb-6 mb-6 border-b border-border">
      <h2 className="text-sm font-semibold text-muted uppercase tracking-wide">
        {title}
      </h2>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-2.5">
        {children}
      </div>
    </section>
  );
}

/** One entry in a `CardSection` — an icon plus its name, the underlying
 * icon/token, and the condition that triggers it. */
function IconCard({
  icon,
  label,
  detail,
}: {
  icon: React.ReactNode;
  label: string;
  detail: string;
}) {
  return (
    <div className="flex flex-col items-center gap-2 text-center rounded-lg border border-border bg-surface px-3.5 py-3.5">
      <span className="flex items-center justify-center size-7">{icon}</span>
      <span className="text-sub font-semibold">{label}</span>
      <span className="text-caption text-muted leading-snug">{detail}</span>
    </div>
  );
}

/** One candidate icon in a comparison row — a small tile labeled with the
 * lucide name (or "current") so a choice can be named unambiguously. */
function IconChoice({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col items-center gap-1.5 rounded-lg border border-border bg-surface px-3 py-3 text-center">
      <span className="flex items-center justify-center size-7">
        {children}
      </span>
      <span className="text-caption text-muted leading-snug">{label}</span>
    </div>
  );
}

/** Renders a comparison row of `IconChoice` tiles from simple label/icon
 * pairs — kept as data so each concept's candidates read as a short list. */
function CandidateRow({
  items,
}: {
  items: { label: string; icon: React.ReactNode }[];
}) {
  return (
    <>
      {items.map((item) => (
        <IconChoice key={item.label} label={item.label}>
          {item.icon}
        </IconChoice>
      ))}
    </>
  );
}

/** Icon candidates for the sidebar's `committed` lifecycle state, which
 * currently reuses `GitPullRequest` (so it differs from `pr_open` only by
 * color). Pick one to replace it. */
const COMMITTED_CANDIDATES: { label: string; icon: React.ReactNode }[] = [
  {
    label: "current · GitPullRequest",
    icon: <GitPullRequest size={16} className="text-muted" />,
  },
  {
    label: "GitCommitVertical",
    icon: <GitCommitVertical size={16} className="text-muted" />,
  },
  {
    label: "GitCommitHorizontal",
    icon: <GitCommitHorizontal size={16} className="text-muted" />,
  },
  {
    label: "CircleDot",
    icon: <CircleDot size={16} className="text-muted" />,
  },
  {
    label: "CircleCheck",
    icon: <CircleCheck size={16} className="text-muted" />,
  },
];

/** Candidates for "a Run-tab command is active", currently a `LassoSpinner`
 * (shared with CI-running below). A non-spinner glyph would let the two be
 * told apart at a glance. */
const RUN_ACTIVE_CANDIDATES: { label: string; icon: React.ReactNode }[] = [
  {
    label: "current · LassoSpinner",
    icon: <LassoSpinner className="size-4" />,
  },
  {
    label: "PlayCircle",
    icon: <PlayCircle size={16} className="text-primary" />,
  },
  { label: "Radio", icon: <Radio size={16} className="text-primary" /> },
  { label: "Server", icon: <Server size={16} className="text-primary" /> },
  { label: "Zap", icon: <Zap size={16} className="text-primary" /> },
  {
    label: "SquareTerminal",
    icon: <SquareTerminal size={16} className="text-primary" />,
  },
];

/** Candidates for "PR checks still running", currently a `LassoSpinner`. */
const CI_RUNNING_CANDIDATES: { label: string; icon: React.ReactNode }[] = [
  {
    label: "current · LassoSpinner",
    icon: <LassoSpinner className="size-4" />,
  },
  {
    label: "CircleDashed",
    icon: <CircleDashed size={16} className="text-warning" />,
  },
  { label: "Activity", icon: <Activity size={16} className="text-warning" /> },
  {
    label: "RefreshCw",
    icon: <RefreshCw size={16} className="text-warning" />,
  },
  {
    label: "LoaderCircle",
    icon: <LoaderCircle size={16} className="text-warning animate-spin" />,
  },
];

/** Candidates for "PR checks failed", currently `CircleX`. */
const CI_FAILED_CANDIDATES: { label: string; icon: React.ReactNode }[] = [
  {
    label: "current · CircleX",
    icon: <CircleX size={16} className="text-danger" />,
  },
  {
    label: "OctagonAlert",
    icon: <OctagonAlert size={16} className="text-danger" />,
  },
  {
    label: "CircleAlert",
    icon: <CircleAlert size={16} className="text-danger" />,
  },
  {
    label: "TriangleAlert",
    icon: <TriangleAlert size={16} className="text-danger" />,
  },
];

/** The full animated-icon family (`spinners.tsx`) plus the two originals —
 * every looping cue in one place, each labeled with the state it's meant
 * for. Pick one per state here. */
const ANIMATED_ICONS: { label: string; icon: React.ReactNode }[] = [
  {
    label: "WindmillSpinner · slow turn — Run active",
    icon: <WindmillSpinner className="size-5" />,
  },
  {
    label: "WagonWheelSpinner · color, wood + iron — Run active",
    icon: <WagonWheelSpinner className="size-5" />,
  },
  {
    label: "WagonWheelSpinnerMono · monochrome fallback",
    icon: <WagonWheelSpinnerMono className="size-5" />,
  },
  {
    label: "GearSpinner · spin — generic busy / Run",
    icon: <GearSpinner className="size-5" />,
  },
  {
    label: "RingSpinner · dashed ring — CI running",
    icon: <RingSpinner className="size-5" />,
  },
  {
    label: "WrenchSpinner · rock — Setup running",
    icon: <WrenchSpinner className="size-5" />,
  },
  {
    label: "BellSpinner · swing — waiting for input",
    icon: <BellSpinner className="size-5" />,
  },
  {
    label: "PulseSpinner · breathe — idle / waiting",
    icon: <PulseSpinner className="size-5" />,
  },
  {
    label: "LassoSpinner · existing",
    icon: <LassoSpinner className="size-5" />,
  },
  { label: "BullSpinner · existing", icon: <BullSpinner /> },
];

const FEATURE_STATUS_DETAIL: Record<FeatureStatus, string> = {
  new: "GitBranch · text-muted — fresh worktree, no commits yet",
  "in-progress": "GitBranchPlus · text-muted — dirty working tree",
  committed: "GitPullRequest · text-muted — clean tree, no PR yet",
  pr_draft: "GitPullRequestDraft · text-muted — draft PR open on GitHub",
  pr_open: "GitPullRequest · text-success — ready-for-review PR open",
  pr_closed: "GitPullRequestClosed · text-danger — PR closed without merging",
  merged: "GitMerge · text-decorative — PR merged / branch merged into base",
  broken: "AlertTriangle · text-danger — not a git repository, safe to remove",
};

/** One example per `PathLabel` rename case — see `splitRenamedPath()`
 * (utils.ts) for how old/new are compared. */
const PATH_LABEL_EXAMPLES: { label: string; path: string; oldPath?: string }[] =
  [
    {
      label: "same directory",
      path: "src/components/new.tsx",
      oldPath: "src/components/old.tsx",
    },
    {
      label: "directory changed",
      path: "src/utils/newHelpers.ts",
      oldPath: "src/lib/oldutils.ts",
    },
    { label: "not a rename", path: "src/components/Foo.tsx" },
  ];

const SYNC_STATES: SyncState[] = ["uncommitted", "committed", "pushed"];

const SYNC_ICON_DETAIL: Record<SyncState, string> = {
  uncommitted:
    "Circle · text-muted — no committed copy at HEAD (new/untracked)",
  committed: "Square · text-warning — committed, not pushed",
  pushed: "SquareArrowUp · text-success — pushed to the remote",
};

const RIGHT_SLOT_DETAIL: Record<RightSlotIndicatorValue["kind"], string> = {
  running:
    "LassoSpinner (rotating) — a Run-tab command is active. Highest priority.",
  attention:
    "🔔 — session waiting for input, not selected, not yet acknowledged.",
  "checks-failed":
    "CircleX · text-danger — open PR has failed/cancelled check runs.",
  "checks-running":
    "LassoSpinner (rotating) — open PR has checks still queued/running.",
  conflict:
    "AlertTriangle · .conflict — would conflict when rebased onto base.",
  diff: ".stat-add / .stat-del — uncommitted diff, or base-diff if the tree is clean.",
  sync: ".ahead / .behind — commits ahead/behind base. Lowest priority.",
  none: "Nothing applies — no indicator shown.",
};

/** Mock worktree + PR data for the PR-page preview below. `PrPane` renders
 * from the zustand caches (and skips its fetch when `active={false}`), so
 * populating them here lets the page be inspected in a plain browser without
 * a Tauri runtime or an actual open PR. */
const PR_PREVIEW_WT: Worktree = {
  project: "corral",
  feature: "keyboard-worktree-switching",
  slug: "keyboard-worktree-switching",
  branch: "feat/keyboard-worktree-switching",
  path: "/home/you/corral/corral/keyboard-worktree-switching",
  status: null,
  has_resumable_session: false,
  note: "",
  title: "Keyboard-driven worktree switching",
};

const PR_PREVIEW_INFO: PrInfo = {
  number: 128,
  url: "https://github.com/nnthnn/corral/pull/128",
  state: "open",
  is_draft: false,
};

const PR_PREVIEW_DETAILS: PrDetails = {
  title: "Add keyboard-driven worktree switching",
  author: "nnthnn",
  // Most PRs have no required approvals — this is the common summary card.
  // Set to "REVIEW_REQUIRED" to preview the Approval-required variant.
  review_decision: "",
  body: [
    "## Summary",
    "- Adds `Ctrl+1–9` worktree jumps and `Ctrl+[` / `Ctrl+]` prev/next.",
    "- Keeps the palette's fuzzy ranking in sync with the sidebar order.",
    "",
    "## Testing",
    "1. Open a project with three or more worktrees.",
    "2. Press `Ctrl+3` — focus should land on the third row.",
    "3. `Ctrl+]` cycles forward, wrapping at the end.",
    "",
    "> Screenshots and a short clip are in the PR description on GitHub.",
  ].join("\n"),
  comments: [
    {
      author: "octocat",
      body: "This feels great. Could we also show the shortcut in the hover card?",
      created_at: "2026-11-12T03:24:00Z",
      url: "https://github.com/nnthnn/corral/pull/128#issuecomment-1",
    },
    {
      author: "github-actions",
      body: "### Build & test\n\nAll checks passed on the latest commit.",
      created_at: "2026-11-12T04:10:00Z",
      url: "https://github.com/nnthnn/corral/pull/128#issuecomment-2",
    },
  ],
};

const PR_PREVIEW_CHECKS: CheckRun[] = [
  {
    name: "build (ubuntu-latest)",
    workflow: "CI",
    bucket: "pass",
    url: "https://github.com/nnthnn/corral/actions",
  },
  {
    name: "clippy",
    workflow: "CI",
    bucket: "pass",
    url: "https://github.com/nnthnn/corral/actions",
  },
  {
    name: "frontend tests",
    workflow: "CI",
    bucket: "pass",
    url: "https://github.com/nnthnn/corral/actions",
  },
  {
    name: "bundle",
    workflow: "Release",
    bucket: "pass",
    url: "https://github.com/nnthnn/corral/actions",
  },
  {
    name: "docs",
    workflow: "Docs",
    bucket: "skipping",
    url: "https://github.com/nnthnn/corral/actions",
  },
];

useStore.setState({
  prInfoByWtId: new Map([[wtId(PR_PREVIEW_WT), PR_PREVIEW_INFO]]),
  prDetailsByWtId: new Map([[wtId(PR_PREVIEW_WT), PR_PREVIEW_DETAILS]]),
  checksByWtId: new Map([[wtId(PR_PREVIEW_WT), PR_PREVIEW_CHECKS]]),
});

function Preview() {
  return (
    <main className="p-6 max-w-6xl mx-auto flex flex-col">
      <h1 className="text-lg font-semibold mb-6">Component Preview</h1>

      <Section title="Button variants">
        {BUTTON_VARIANTS.map((variant) => (
          <Button key={variant} variant={variant}>
            {variant}
          </Button>
        ))}
      </Section>

      <Section title="Button — icon / square / active">
        <Button icon>icon</Button>
        <Button square aria-label="square">
          #
        </Button>
        <Button variant="primary" active>
          active
        </Button>
      </Section>

      <CardSection title="FeatureStatusIcon">
        {FEATURE_STATUSES.map((status) => (
          <IconCard
            key={status}
            icon={<FeatureStatusIcon status={status} />}
            label={status}
            detail={FEATURE_STATUS_DETAIL[status]}
          />
        ))}
      </CardSection>

      <CardSection title="RightSlotIndicator (priority order, top wins)">
        {RIGHT_SLOT_INDICATORS.map(({ label, indicator }) => (
          <IconCard
            key={label}
            icon={<RightSlotIndicator indicator={indicator} />}
            label={label}
            detail={RIGHT_SLOT_DETAIL[indicator.kind]}
          />
        ))}
      </CardSection>

      <CardSection title="Icon candidates — 'committed' (pick one)">
        <CandidateRow items={COMMITTED_CANDIDATES} />
      </CardSection>

      <CardSection title="Icon candidates — Run active (pick one)">
        <CandidateRow items={RUN_ACTIVE_CANDIDATES} />
      </CardSection>

      <CardSection title="Icon candidates — CI running (pick one)">
        <CandidateRow items={CI_RUNNING_CANDIDATES} />
      </CardSection>

      <CardSection title="Icon candidates — CI failed (pick one)">
        <CandidateRow items={CI_FAILED_CANDIDATES} />
      </CardSection>

      <CardSection title="Animated icons (all looping — pick one per state)">
        <CandidateRow items={ANIMATED_ICONS} />
      </CardSection>

      <Section title="PathLabel (Changes pane — renamed-file display)">
        <div className="flex w-85 flex-col gap-2 rounded-lg border border-border bg-surface p-2">
          {PATH_LABEL_EXAMPLES.map(({ label, path, oldPath }) => (
            <div key={label} className="flex flex-col gap-0.5">
              <span className="text-caption text-muted">{label}</span>
              <span className="font-mono text-caption">
                <PathLabel path={path} oldPath={oldPath} />
              </span>
            </div>
          ))}
        </div>
      </Section>

      <CardSection title="SyncIcon (Changes pane — per-file push state)">
        {SYNC_STATES.map((state) => (
          <IconCard
            key={state}
            icon={<SyncIcon state={state} />}
            label={state}
            detail={SYNC_ICON_DETAIL[state]}
          />
        ))}
        {SYNC_STATES.filter((state) => state !== "uncommitted").map((state) => (
          <IconCard
            key={`${state}-dirty`}
            icon={<SyncIcon state={state} dirty />}
            label={`${state} + local edits`}
            detail="same icon with a dot — committed/pushed, but edited again locally"
          />
        ))}
      </CardSection>

      <Section title="ProjectIcon">
        <ProjectIcon project="corral" collapsed={false} />
      </Section>

      <Section title="Highlight">
        <Highlight text="feat/tilling-worktree" needle="tilling" />
      </Section>

      <section className="flex flex-col gap-3 pb-6 mb-6 border-b border-border">
        <h2 className="text-sm font-semibold text-muted uppercase tracking-wide">
          PR page (mock data)
        </h2>
        <p className="text-caption text-muted leading-snug">
          Rendered from the zustand caches with no Tauri calls. Expand{" "}
          <code>Checks</code> or follow the <code>Comments</code> link to the
          timeline; GitHub actions (open, mark ready, add to chat) are inert
          here.
        </p>
        <div className="h-[760px] w-full overflow-hidden rounded-lg border border-border bg-bg">
          <PrPane wt={PR_PREVIEW_WT} active={false} />
        </div>
      </section>
    </main>
  );
}

const container = document.getElementById("root");
if (!container) throw new Error("#root element missing from preview.html");

createRoot(container).render(
  <StrictMode>
    <Preview />
  </StrictMode>,
);
