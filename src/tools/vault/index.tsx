import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DecryptPanel from "./DecryptPanel";
import EncryptPanel from "./EncryptPanel";
import "./vault.css";

type Mode = "encrypt" | "decrypt";

/** Quantum Resistance Vault: encrypt large files with a passphrase of random
 * words, and decrypt them again. All crypto runs in Rust; this screen only
 * shows state. Both tabs stay mounted so nothing is lost when switching. */
export default function Vault() {
  const [mode, setMode] = useState<Mode>("encrypt");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

  const handleBusy = useCallback((value: boolean) => {
    busyRef.current = value;
    setBusy(value);
  }, []);

  // Leaving the tool: stop a running operation and wipe the passphrase.
  useEffect(() => {
    return () => {
      if (busyRef.current) {
        void invoke("vault_cancel").catch(() => undefined);
      }
      void invoke("vault_clear_passphrase").catch(() => undefined);
    };
  }, []);

  return (
    <div className="vt">
      <p className="vt-intro">
        Encrypt files with a passphrase of random words. The design is meant to resist
        quantum attacks. Everything happens on this computer, and the passphrase is
        never saved.
      </p>
      <div className="vt-tabs" role="tablist" aria-label="Vault mode">
        <button
          type="button"
          role="tab"
          aria-selected={mode === "encrypt"}
          className={mode === "encrypt" ? "vt-tab vt-tab-active" : "vt-tab"}
          onClick={() => setMode("encrypt")}
          disabled={busy}
        >
          Encrypt
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={mode === "decrypt"}
          className={mode === "decrypt" ? "vt-tab vt-tab-active" : "vt-tab"}
          onClick={() => setMode("decrypt")}
          disabled={busy}
        >
          Decrypt
        </button>
      </div>
      <EncryptPanel active={mode === "encrypt"} onBusyChange={handleBusy} />
      <DecryptPanel active={mode === "decrypt"} onBusyChange={handleBusy} />
    </div>
  );
}
