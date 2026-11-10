import clsx from "clsx";
import { X } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { closeTab } from "../lib/actions";
import { checksArePending } from "../lib/checks";
import {
  currentWt,
  sessionStatus,
  type TabId,
  useStore,
  worktreeTabIds,
} from "../store";
import { wtId } from "../utils";
import { BullSpinner } from "./BullSpinner";
import { LassoSpinner } from "./LassoSpinner";
import { WagonWheelSpinner } from "./spinners";

const NAV_CLASS =
  "tab-bar flex gap-1 pt-1.5 px-3 border-b border-border bg-surface";

const TAB_LABEL: Record<Exclude<TabId, "pr">, string> = {
  agent: "Agent",
  run: "Run",
  setup: "Setup",
  diff: "Diff",
  file: "File",
};

export function TabBar() {
  const wt = useStore(currentWt);
  const tabIds = useStore(useShallow((s) => worktreeTabIds(s)));
  const activeTab = useStore((s) => s.activeTab);
  const working = useStore(
    (s) => !!wt && sessionStatus(s, wtId(wt))?.cls === "working",
  );
  const prInfo =
    useStore((s) => (wt ? s.prInfoByWtId.get(wtId(wt)) : null)) ?? null;
  const prChecksPending = useStore((s) =>
    wt ? checksArePending(s.checksByWtId.get(wtId(wt))) : false,
  );
  const runRunning = useStore(
    (s) => !!wt && s.run.get(wtId(wt))?.running === true,
  );
  const setupRunning = useStore(
    (s) => !!wt && s.provisioning.get(wtId(wt))?.running === true,
  );

  if (tabIds.length <= 1) return null;

  return (
    <nav className={NAV_CLASS}>
      {tabIds.map((id) => {
        const closable = id === "diff" || id === "file";
        return (
          // biome-ignore lint/a11y/useSemanticElements: nests the real close <button> below, invalid inside a <button>
          <div
            key={id}
            className={clsx("tab", id === activeTab && "active")}
            role="button"
            tabIndex={0}
            onClick={() => useStore.setState({ activeTab: id })}
            onKeyDown={(e) => {
              // Ignore keys bubbled from the nested close button — otherwise
              // our preventDefault() cancels its Enter/Space activation.
              if (e.target !== e.currentTarget) return;
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                useStore.setState({ activeTab: id });
              }
            }}
          >
            <span className="tab-label inline-flex items-center gap-1.5">
              {id === "agent" && working && <BullSpinner />}
              {id === "run" && runRunning && (
                <WagonWheelSpinner className="size-3.5" />
              )}
              {id === "setup" && setupRunning && (
                <LassoSpinner className="size-3.5" />
              )}
              {id === "pr" && prChecksPending && (
                <LassoSpinner className="size-3.5" />
              )}
              {id === "pr" ? `PR #${prInfo?.number}` : TAB_LABEL[id]}
            </span>
            {closable && (
              <button
                type="button"
                className="tab-close bg-transparent border-none cursor-pointer opacity-60 rounded-sm px-1 flex items-center hover:opacity-100 hover:bg-surface-2/80 ml-2 -mr-3"
                title="Close tab"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(id);
                }}
              >
                <X size={13} />
              </button>
            )}
          </div>
        );
      })}
    </nav>
  );
}
