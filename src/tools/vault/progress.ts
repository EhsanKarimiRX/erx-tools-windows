import { useCallback, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import type { VaultEvent } from "./types";

/** What the screen is doing right now. */
export type Stage = "idle" | "tuning" | "deriving" | "working" | "verifying";

export interface ProgressView {
  stage: Stage;
  done: number;
  total: number;
  bytesPerSecond: number | null;
  etaSeconds: number | null;
}

const IDLE_VIEW: ProgressView = {
  stage: "idle",
  done: 0,
  total: 0,
  bytesPerSecond: null,
  etaSeconds: null,
};

/** Speed and time left need a little history before they are worth showing. */
const MIN_SPEED_WINDOW_SECONDS = 1.5;

/** Progress state for one vault operation. `start` returns the channel to
 * pass to the Rust command; `reset` goes back to idle when it is over. */
export function useVaultProgress() {
  const [view, setView] = useState<ProgressView>(IDLE_VIEW);
  const stageRef = useRef<Stage>("idle");
  const baselineRef = useRef<{ at: number; done: number } | null>(null);

  const reset = useCallback(() => {
    stageRef.current = "idle";
    baselineRef.current = null;
    setView(IDLE_VIEW);
  }, []);

  const start = useCallback((): Channel<VaultEvent> => {
    stageRef.current = "deriving";
    baselineRef.current = null;
    setView({ ...IDLE_VIEW, stage: "deriving" });
    const channel = new Channel<VaultEvent>();
    channel.onmessage = (event) => {
      // Late messages after the operation ended are ignored.
      if (stageRef.current === "idle") return;
      if (event.event === "phase") {
        stageRef.current = event.phase;
        baselineRef.current = null;
        setView({
          stage: event.phase,
          done: 0,
          total: 0,
          bytesPerSecond: null,
          etaSeconds: null,
        });
        return;
      }
      if (stageRef.current === "tuning" || stageRef.current === "deriving") {
        stageRef.current = "working";
      }
      const now = performance.now();
      if (baselineRef.current === null) {
        baselineRef.current = { at: now, done: event.done };
      }
      const elapsedSeconds = (now - baselineRef.current.at) / 1000;
      const moved = event.done - baselineRef.current.done;
      const speed =
        elapsedSeconds >= MIN_SPEED_WINDOW_SECONDS && moved > 0
          ? moved / elapsedSeconds
          : null;
      const remaining = Math.max(0, event.total - event.done);
      setView({
        stage: stageRef.current,
        done: event.done,
        total: event.total,
        bytesPerSecond: speed,
        etaSeconds: speed !== null ? remaining / speed : null,
      });
    };
    return channel;
  }, []);

  return { view, start, reset };
}
