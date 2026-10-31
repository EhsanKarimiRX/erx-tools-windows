/**
 * Minimal example tool. To start a new tool, copy this folder to
 * `src/tools/<your-tool-id>/`, adjust it, and register it in
 * `src/tools/registry.ts` (full recipe: docs/ADDING_A_TOOL.md).
 *
 * Tool components only render their content; the shared header with the
 * Back button and title comes from ToolPage via ToolLayout.
 */
export default function TemplateTool() {
  return <p className="tool-placeholder">This tool is not built yet.</p>;
}
