import type { ComponentType } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Folder,
  Globe,
  Image as ImageIcon,
  MonitorCog,
  NotebookPen,
  QrCode,
} from "lucide-react";

/** Lifecycle status of a tool; controls how its home tile behaves. */
export type ToolStatus =
  /** The tile opens the tool's screen (a placeholder screen counts). */
  | "ready"
  /** The tile is dimmed and cannot be clicked. */
  | "coming-soon";

/** A single entry in the tool registry. */
export interface ToolDefinition {
  /** Unique, stable id; also used for navigation. */
  id: string;
  /** Human-readable name shown on the home tile. */
  name: string;
  /** One-line description, shown as the tile tooltip. */
  description: string;
  /** Icon shown on the home tile. */
  icon: LucideIcon;
  /** Lazy component loader; the tool's code is only fetched when opened. */
  load: () => Promise<{ default: ComponentType }>;
  status: ToolStatus;
}

// The home screen is generated from this list and nothing else.
// To add a tool: create src/tools/<id>/ and add one entry here.
// Full recipe: docs/ADDING_A_TOOL.md
export const tools: readonly ToolDefinition[] = [
  {
    id: "image-converter",
    name: "Image Converter",
    description: "Convert images between common formats",
    icon: ImageIcon,
    load: () => import("./image-converter"),
    status: "ready",
  },
  {
    id: "qr-code",
    name: "QR Code Generator",
    description: "Create QR codes from text or links",
    icon: QrCode,
    load: () => import("./qr-code"),
    status: "ready",
  },
  {
    id: "favicons",
    name: "Favicons",
    description: "Create favicon and web app icons from your logo",
    icon: Globe,
    load: () => import("./favicons"),
    status: "ready",
  },
  {
    id: "folder-icon",
    name: "Folder Icon Changer",
    description: "Change the icon of a folder",
    icon: Folder,
    load: () => import("./folder-icon"),
    status: "ready",
  },
  {
    id: "notes",
    name: "Encrypted Notes",
    description: "Keep private notes, encrypted on disk",
    icon: NotebookPen,
    load: () => import("./notes"),
    status: "ready",
  },
  {
    id: "system-manager",
    name: "System Manager",
    description: "Awake, desktop icons, organizer and quick launchers.",
    icon: MonitorCog,
    load: () => import("./system-manager"),
    status: "ready",
  },
];
