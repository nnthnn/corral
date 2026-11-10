import clsx from "clsx";
import { useEffect } from "react";
import { currentWt, useStore } from "../store";
import { AgentPane } from "./AgentPane";
import { DiffViewer } from "./DiffViewer";
import { FilePreview } from "./FilePreview";
import { MainHead } from "./MainHead";
import { ProvisionPanel } from "./ProvisionPanel";
import { PrPane } from "./PrPane";
import { RunPanel } from "./RunPanel";
import { SettingsPanel } from "./SettingsPanel";
import { TabBar } from "./TabBar";

export function MainPane() {
  const activeTab = useStore((s) => s.activeTab);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const wt = useStore(currentWt);
  const reselectFlash = useStore((s) => s.reselectFlash);

  useEffect(() => {
    if (!reselectFlash) return;
    const t = setTimeout(() => {
      useStore.setState({ reselectFlash: false });
    }, 150);
    return () => clearTimeout(t);
  }, [reselectFlash]);

  return (
    <main
      className={clsx(
        "main flex flex-col relative min-w-0 min-h-0",
        reselectFlash && "reselect-flash",
      )}
    >
      <MainHead />
      <TabBar />
      <div className="main-body relative flex-1 min-h-0">
        {/* Explicitly hidden when settings is open — settings-pane's DOM
         * position alone isn't reliable insurance: xterm's own render layers
         * can end up stacked above it and swallow clicks. */}
        <AgentPane hidden={activeTab !== "agent" || settingsOpen} />
        <div
          id="run-pane"
          className="tab-pane absolute inset-0"
          hidden={activeTab !== "run" || settingsOpen}
        >
          {wt && <RunPanel wt={wt} />}
        </div>
        <div
          id="setup-pane"
          className="tab-pane absolute inset-0"
          hidden={activeTab !== "setup" || settingsOpen}
        >
          {wt && <ProvisionPanel wt={wt} />}
        </div>
        <div
          id="diff-pane"
          className="tab-pane absolute inset-0"
          hidden={activeTab !== "diff" || settingsOpen}
        >
          <DiffViewer />
        </div>
        <div
          id="file-pane"
          className="tab-pane absolute inset-0"
          hidden={activeTab !== "file" || settingsOpen}
        >
          <FilePreview />
        </div>
        <div
          id="pr-pane"
          className="tab-pane absolute inset-0"
          hidden={activeTab !== "pr" || settingsOpen}
        >
          {wt && (
            <PrPane wt={wt} active={activeTab === "pr" && !settingsOpen} />
          )}
        </div>
        <SettingsPanel />
      </div>
    </main>
  );
}
