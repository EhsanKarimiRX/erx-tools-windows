import AwakeSection from "./AwakeSection";
import CommandTipsSection from "./CommandTipsSection";
import DesktopIconsSection from "./DesktopIconsSection";
import OrganizeSection from "./OrganizeSection";
import QuickLaunchSection from "./QuickLaunchSection";
import "./system-manager.css";

/** System Manager: one scrolling page of section cards, in the owner's
 * fixed order: Awake, Desktop Icons, Organize, Quick Launch, Command Tips.
 * Only content is rendered here — the Back + title header comes from
 * ToolPage via ToolLayout. */
export default function SystemManager() {
  return (
    <div className="sm">
      <AwakeSection />
      <DesktopIconsSection />
      <OrganizeSection />
      <QuickLaunchSection />
      <CommandTipsSection />
    </div>
  );
}
