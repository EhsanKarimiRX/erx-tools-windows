import AwakeSection from "./AwakeSection";
import DesktopIconsSection from "./DesktopIconsSection";
import "./system-manager.css";

/** System Manager: one scrolling page of section cards (Awake, Desktop
 * Icons, Organize, Quick Launch, Command Tips), built section by section.
 * Only content is rendered here — the Back + title header comes from
 * ToolPage via ToolLayout. The placeholder line at the bottom is removed
 * in step 5g when the page is complete. */
export default function SystemManager() {
  return (
    <div className="sm">
      <AwakeSection />
      <DesktopIconsSection />
      <p className="tool-placeholder">System Manager is being built section by section.</p>
    </div>
  );
}
