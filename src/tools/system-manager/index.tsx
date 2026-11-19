import AwakeSection from "./AwakeSection";
import CommandTipsSection from "./CommandTipsSection";
import QuickLaunchSection from "./QuickLaunchSection";
import SystemInfoCard from "./SystemInfoCard";
import "./system-manager.css";

/** System Manager: one scrolling page of section cards, in the owner's
 * fixed order: the device card on top, then Awake, Quick Launch,
 * Command Tips.
 * Only content is rendered here — the Back + title header comes from
 * ToolPage via ToolLayout. */
export default function SystemManager() {
  return (
    <div className="sm">
      <SystemInfoCard />
      <AwakeSection />
      <QuickLaunchSection />
      <CommandTipsSection />
    </div>
  );
}
