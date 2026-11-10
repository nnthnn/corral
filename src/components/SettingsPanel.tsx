import { useStore } from "../store";
import { SettingsForm } from "./SettingsForm";

export function SettingsPanel() {
  const settingsOpen = useStore((s) => s.settingsOpen);

  return (
    <div
      id="settings-pane"
      className="tab-pane absolute inset-0"
      hidden={!settingsOpen}
    >
      {settingsOpen && <SettingsForm />}
    </div>
  );
}
