import { useState } from "react";
import { tools } from "../tools/registry";
import HomeScreen from "./HomeScreen";
import ToolPage from "./ToolPage";

/**
 * Root shell of the app: a tiny state-based view switch between the home
 * screen and one open tool. No router needed at this size.
 */
export default function App() {
  const [activeToolId, setActiveToolId] = useState<string | null>(null);

  const activeTool =
    activeToolId === null
      ? null
      : (tools.find((tool) => tool.id === activeToolId) ?? null);

  if (activeTool === null) {
    return <HomeScreen onOpenTool={setActiveToolId} />;
  }

  return <ToolPage tool={activeTool} onBack={() => setActiveToolId(null)} />;
}
