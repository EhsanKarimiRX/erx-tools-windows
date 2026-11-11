import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { tools } from "../tools/registry";
import UpdateBadge from "./UpdateBadge";
import AboutButton from "./AboutButton";

interface AppInfo {
  name: string;
  version: string;
}

/** App home: header, the tile grid generated from the registry, and footer. */
export default function HomeScreen({
  onOpenTool,
}: {
  onOpenTool: (toolId: string) => void;
}) {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    // Fetch the version from Rust; also proves the frontend-to-Rust bridge.
    // In a plain browser (vite only) there is no bridge; footer stays hidden.
    let cancelled = false;
    invoke<AppInfo>("get_app_info")
      .then((info) => {
        if (!cancelled) setVersion(info.version);
      })
      .catch(() => {
        /* no Tauri bridge available (plain browser dev) */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="home">
      <header className="home-header">
        <h1>ERX Tools</h1>
        <p className="home-subtitle">Fast, private, offline tools</p>
      </header>

      <main className="tiles" aria-label="Tools">
        {tools.map((tool) => {
          const Icon = tool.icon;
          const isReady = tool.status === "ready";
          return (
            <button
              key={tool.id}
              type="button"
              className="tile"
              title={
                isReady
                  ? tool.description
                  : `${tool.description} (coming soon)`
              }
              disabled={!isReady}
              onClick={() => onOpenTool(tool.id)}
            >
              <Icon size={48} strokeWidth={1.5} aria-hidden="true" />
              <span className="tile-name">{tool.name}</span>
              {!isReady && <span className="tile-badge">Coming soon</span>}
            </button>
          );
        })}
      </main>

      <footer className="home-footer">
        {version !== null && <UpdateBadge version={version} />}
      </footer>

      <AboutButton version={version} />
    </div>
  );
}
