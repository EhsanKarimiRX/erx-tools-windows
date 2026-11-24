/** The notes screen: the note list on the left, the editor on the right.
 * Edits are debounced and sent to Rust; the shell owns the save indicator. */
import { useEffect, useRef, useState } from "react";
import {
  Copy,
  Download,
  Plus,
  Redo2,
  Save,
  Trash2,
  Undo2,
} from "lucide-react";
import {
  copyText,
  createNote,
  deleteNote,
  exportNote,
  getNote,
  listNotes,
  saveNow,
  updateNote,
} from "../api";
import { toApiError } from "../errors";
import type { NoteListItem } from "../types";

const SEARCH_DEBOUNCE_MS = 200;
const UPDATE_DEBOUNCE_MS = 400;
const COPIED_MS = 2_000;
const EXPORT_STATUS_MS = 2_000;

type ExportFormat = "txt" | "md" | "erxnote";

interface PendingUpdate {
  id: string;
  title: string;
  body: string;
}

/** One whole-note state in the undo stack: the title and body together. */
interface NoteSnapshot {
  title: string;
  body: string;
}

interface NotesScreenProps {
  /** Called on every editor change so the shell can show "Saving...". */
  onDirty: () => void;
}

export default function NotesScreen({ onDirty }: NotesScreenProps) {
  const [notes, setNotes] = useState<NoteListItem[]>([]);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [loadingList, setLoadingList] = useState(true);
  const [loadingNote, setLoadingNote] = useState(false);
  const [working, setWorking] = useState(false);
  const [saveWorking, setSaveWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copiedUntil, setCopiedUntil] = useState<number | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const [history, setHistory] = useState<NoteSnapshot[]>([]);
  const [historyIndex, setHistoryIndex] = useState(0);

  const searchTimer = useRef<number | null>(null);
  const exportTimer = useRef<number | null>(null);
  const updateTimer = useRef<number | null>(null);
  const selectedIdRef = useRef<string | null>(null);
  const queryRef = useRef("");
  const pendingRef = useRef<PendingUpdate | null>(null);
  const noteSeq = useRef(0);
  const titleRef = useRef<HTMLInputElement>(null);
  const exportRef = useRef<HTMLDivElement>(null);
  const focusTitle = useRef(false);
  const newNoteRef = useRef<() => void>(() => {});
  // Mirrors of the undo stack so two edits in one frame cannot read stale
  // state; the state copies drive the buttons' disabled props.
  const historyRef = useRef<NoteSnapshot[]>([]);
  const historyIndexRef = useRef(0);

  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  useEffect(() => {
    queryRef.current = query;
  }, [query]);

  // List: load on mount and after every search change, debounced.
  useEffect(() => {
    let cancelled = false;
    if (searchTimer.current !== null) window.clearTimeout(searchTimer.current);
    setLoadingList(true);
    searchTimer.current = window.setTimeout(() => {
      searchTimer.current = null;
      listNotes(query).then(
        (items) => {
          if (cancelled) return;
          setNotes(items);
          setLoadingList(false);
        },
        (loadError: unknown) => {
          if (cancelled) return;
          setError(toApiError(loadError).message);
          setLoadingList(false);
        },
      );
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      if (searchTimer.current !== null) {
        window.clearTimeout(searchTimer.current);
        searchTimer.current = null;
      }
    };
  }, [query]);

  // Focus the title of a note that was just created.
  useEffect(() => {
    if (selectedId !== null && focusTitle.current) {
      focusTitle.current = false;
      titleRef.current?.focus();
    }
  }, [selectedId]);

  // Hide the copy confirmation after two seconds.
  useEffect(() => {
    if (copiedUntil === null) return;
    const remaining = Math.max(0, copiedUntil - Date.now());
    const timer = window.setTimeout(() => setCopiedUntil(null), remaining);
    return () => window.clearTimeout(timer);
  }, [copiedUntil]);

  // Drop the export confirmation timer when the screen goes away.
  useEffect(() => {
    return () => {
      if (exportTimer.current !== null) {
        window.clearTimeout(exportTimer.current);
        exportTimer.current = null;
      }
    };
  }, []);

  // Close the export menu on a click outside it. Escape is handled on the
  // menu wrapper so it only closes while the menu is open.
  useEffect(() => {
    if (!exportOpen) return;
    const onMouseDown = (event: MouseEvent) => {
      const wrapper = exportRef.current;
      if (
        wrapper !== null &&
        event.target instanceof Node &&
        wrapper.contains(event.target)
      ) {
        return;
      }
      setExportOpen(false);
    };
    document.addEventListener("mousedown", onMouseDown);
    return () => document.removeEventListener("mousedown", onMouseDown);
  }, [exportOpen]);

  // Send an edit that is still waiting in the debounce before the window
  // closes or locks. This is fire-and-forget: the window may be going away.
  useEffect(() => {
    return () => {
      if (updateTimer.current !== null) {
        window.clearTimeout(updateTimer.current);
        updateTimer.current = null;
      }
      const pending = pendingRef.current;
      pendingRef.current = null;
      if (pending !== null) {
        void updateNote(pending.id, pending.title, pending.body).catch(() => {
          // The shell's autosave event reports failures while it is open.
        });
      }
    };
  }, []);

  // Ctrl+N creates a note. The handler is kept in a ref so the listener can be
  // registered once.
  useEffect(() => {
    newNoteRef.current = () => {
      void handleNew();
    };
  });
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === "n"
      ) {
        event.preventDefault();
        newNoteRef.current();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  function clearExportTimer() {
    if (exportTimer.current !== null) {
      window.clearTimeout(exportTimer.current);
      exportTimer.current = null;
    }
  }

  /** Shows the export result; a null delay keeps the message until the next
   * export or note switch. */
  function showExportStatus(message: string, clearAfter: number | null) {
    clearExportTimer();
    setExportMessage(message);
    if (clearAfter !== null) {
      exportTimer.current = window.setTimeout(() => {
        exportTimer.current = null;
        setExportMessage(null);
      }, clearAfter);
    }
  }

  async function refreshList(search: string): Promise<void> {
    try {
      const items = await listNotes(search);
      setNotes(items);
    } catch (refreshError) {
      setError(toApiError(refreshError).message);
    }
  }

  /** Sends the debounced edit now, if any. Returns false when it failed. */
  async function flushPending(): Promise<boolean> {
    if (updateTimer.current !== null) {
      window.clearTimeout(updateTimer.current);
      updateTimer.current = null;
    }
    const pending = pendingRef.current;
    if (pending === null) return true;
    pendingRef.current = null;
    setSaveWorking(true);
    try {
      await updateNote(pending.id, pending.title, pending.body);
      await refreshList(queryRef.current);
      return true;
    } catch (updateError) {
      setError(toApiError(updateError).message);
      return false;
    } finally {
      setSaveWorking(false);
    }
  }

  function scheduleUpdate(nextTitle: string, nextBody: string) {
    const id = selectedIdRef.current;
    if (id === null) return;
    pendingRef.current = { id, title: nextTitle, body: nextBody };
    if (updateTimer.current !== null) window.clearTimeout(updateTimer.current);
    updateTimer.current = window.setTimeout(() => {
      updateTimer.current = null;
      void flushPending();
    }, UPDATE_DEBOUNCE_MS);
  }

  /** Publishes a history stack to both the ref and the rendered state. */
  function commitHistory(entries: NoteSnapshot[], index: number) {
    historyRef.current = entries;
    historyIndexRef.current = index;
    setHistory(entries);
    setHistoryIndex(index);
  }

  /** Starts a fresh undo history for a note that was just opened. */
  function resetHistory(nextTitle: string, nextBody: string) {
    commitHistory([{ title: nextTitle, body: nextBody }], 0);
  }

  /** Records one whole-note edit and drops everything after the current
   * position, so the redo branch belongs to the old text only. */
  function recordHistory(nextTitle: string, nextBody: string) {
    const current = historyRef.current;
    const index = historyIndexRef.current;
    const top = current[index];
    if (top !== undefined && top.title === nextTitle && top.body === nextBody) {
      return;
    }
    const entries = current.slice(0, index + 1);
    entries.push({ title: nextTitle, body: nextBody });
    commitHistory(entries, entries.length - 1);
  }

  /** Puts an undo/redo snapshot back in the editor and queues its save. */
  function applyHistorySnapshot(snapshot: NoteSnapshot) {
    setTitle(snapshot.title);
    setBody(snapshot.body);
    onDirty();
    scheduleUpdate(snapshot.title, snapshot.body);
  }

  function handleUndo() {
    if (loadingNote) return;
    const index = historyIndexRef.current;
    if (index <= 0) return;
    const snapshot = historyRef.current[index - 1];
    if (snapshot === undefined) return;
    commitHistory(historyRef.current, index - 1);
    applyHistorySnapshot(snapshot);
  }

  function handleRedo() {
    if (loadingNote) return;
    const index = historyIndexRef.current;
    if (index >= historyRef.current.length - 1) return;
    const snapshot = historyRef.current[index + 1];
    if (snapshot === undefined) return;
    commitHistory(historyRef.current, index + 1);
    applyHistorySnapshot(snapshot);
  }

  function handleTitleChange(value: string) {
    setTitle(value);
    onDirty();
    scheduleUpdate(value, body);
    recordHistory(value, body);
  }

  function handleBodyChange(value: string) {
    setBody(value);
    onDirty();
    scheduleUpdate(title, value);
    recordHistory(title, value);
  }

  async function selectNote(id: string) {
    // Save the previous note's last keystrokes before switching.
    void flushPending();
    const seq = noteSeq.current + 1;
    noteSeq.current = seq;
    selectedIdRef.current = id;
    setSelectedId(id);
    setTitle("");
    setBody("");
    resetHistory("", "");
    setCopiedUntil(null);
    setExportOpen(false);
    setExportMessage(null);
    clearExportTimer();
    setError(null);
    setLoadingNote(true);
    try {
      const note = await getNote(id);
      if (seq !== noteSeq.current) return;
      setTitle(note.title);
      setBody(note.body);
      resetHistory(note.title, note.body);
    } catch (loadError) {
      if (seq !== noteSeq.current) return;
      setError(toApiError(loadError).message);
    } finally {
      if (seq === noteSeq.current) setLoadingNote(false);
    }
  }

  async function handleNew() {
    if (working) return;
    setWorking(true);
    setError(null);
    await flushPending();
    try {
      const note = await createNote("", "");
      noteSeq.current += 1;
      selectedIdRef.current = note.id;
      setSelectedId(note.id);
      setTitle(note.title);
      setBody(note.body);
      resetHistory(note.title, note.body);
      setLoadingNote(false);
      setCopiedUntil(null);
      setExportOpen(false);
      setExportMessage(null);
      clearExportTimer();
      focusTitle.current = true;
      if (queryRef.current !== "") {
        setQuery("");
        queryRef.current = "";
      }
      await refreshList("");
    } catch (createError) {
      setError(toApiError(createError).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleDelete() {
    const id = selectedIdRef.current;
    if (id === null || working) return;
    setWorking(true);
    setError(null);
    // Drop any pending edit for the note that is about to be deleted.
    if (updateTimer.current !== null) {
      window.clearTimeout(updateTimer.current);
      updateTimer.current = null;
    }
    pendingRef.current = null;
    noteSeq.current += 1;
    try {
      await deleteNote(id);
      selectedIdRef.current = null;
      setSelectedId(null);
      setTitle("");
      setBody("");
      resetHistory("", "");
      setLoadingNote(false);
      setCopiedUntil(null);
      setExportOpen(false);
      setExportMessage(null);
      clearExportTimer();
      const items = await listNotes(queryRef.current);
      setNotes(items);
      if (items.length > 0) {
        await selectNote(items[0].id);
      }
    } catch (deleteError) {
      setError(toApiError(deleteError).message);
    } finally {
      setWorking(false);
    }
  }

  /** Deletes the note behind one list row. The delete flow works on the
   * selected note, so select the row first when it is not already open. */
  async function handleDeleteItem(id: string) {
    if (id !== selectedIdRef.current) {
      await selectNote(id);
    }
    await handleDelete();
  }

  async function handleSaveNow() {
    if (working) return;
    setWorking(true);
    setError(null);
    try {
      const flushed = await flushPending();
      if (!flushed) return;
      await saveNow();
    } catch (saveError) {
      setError(toApiError(saveError).message);
    } finally {
      setWorking(false);
    }
  }

  const copyValue = body.trim() !== "" ? body : title;
  const canCopy = copyValue.trim() !== "";

  async function handleCopy() {
    if (!canCopy || working) return;
    setWorking(true);
    setError(null);
    try {
      await copyText(copyValue);
      setCopiedUntil(Date.now() + COPIED_MS);
    } catch (copyError) {
      setError(toApiError(copyError).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleExport(format: ExportFormat) {
    const id = selectedIdRef.current;
    setExportOpen(false);
    if (id === null || working) return;
    setWorking(true);
    setError(null);
    try {
      // Export what is on screen, not the last debounced save.
      const flushed = await flushPending();
      if (!flushed) return;
      const path = await exportNote(id, format);
      if (path === null) {
        showExportStatus("Export cancelled.", EXPORT_STATUS_MS);
      } else {
        showExportStatus(`Exported to ${path}.`, EXPORT_STATUS_MS);
      }
    } catch (exportError) {
      setError(toApiError(exportError).message);
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="nw-notes">
      <section className="nw-list-pane" aria-label="Notes list">
        <div className="nw-pane-head">
          <h2 className="nw-pane-title">List of notes</h2>
          {selectedId !== null && (
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => void handleNew()}
              disabled={working}
            >
              New note
            </button>
          )}
        </div>
        <input
          type="search"
          className="nw-search"
          placeholder="Search titles"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          dir="auto"
          spellCheck={false}
          autoComplete="off"
          aria-label="Search notes by title"
        />
        {loadingList && notes.length === 0 ? (
          <p className="nw-muted nw-list-empty">Loading notes...</p>
        ) : notes.length === 0 ? (
          <p className="nw-muted nw-list-empty">
            {query.trim() === ""
              ? "No notes yet. Create one to start."
              : "No notes match this search."}
          </p>
        ) : (
          <ul className="nw-list">
            {notes.map((note) => {
              const label =
                note.title.trim() === "" ? "(untitled)" : note.title;
              return (
                <li key={note.id}>
                  <div
                    className={
                      note.id === selectedId
                        ? "nw-list-item selected"
                        : "nw-list-item"
                    }
                  >
                    <button
                      type="button"
                      className="nw-list-item-body"
                      onClick={() => void selectNote(note.id)}
                      aria-current={note.id === selectedId ? "true" : undefined}
                    >
                      <span className="nw-list-title" dir="auto">
                        {label}
                      </span>
                      {note.preview !== null && note.preview !== "" && (
                        <span className="nw-list-preview" dir="auto">
                          {note.preview}
                        </span>
                      )}
                      <span className="nw-list-date">
                        {new Date(note.modified * 1000).toLocaleDateString()}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="nw-list-delete"
                      onClick={() => void handleDeleteItem(note.id)}
                      disabled={working}
                      title="Delete note"
                      aria-label={`Delete ${label}`}
                    >
                      <Trash2 size={14} aria-hidden="true" />
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section className="nw-editor-pane nw-editor" aria-label="Note editor">
        {selectedId === null ? (
          <div className="nw-empty">
            <p>Select a note or create a new one.</p>
            <button
              type="button"
              className="nw-button-primary nw-empty-cta"
              onClick={() => void handleNew()}
              disabled={working}
            >
              <Plus size={14} aria-hidden="true" />
              New note
            </button>
          </div>
        ) : (
          <>
            <input
              ref={titleRef}
              type="text"
              className="nw-editor-title"
              value={title}
              onChange={(event) => handleTitleChange(event.target.value)}
              placeholder="Title"
              dir="auto"
              spellCheck={false}
              aria-label="Note title"
              disabled={loadingNote}
            />
            <textarea
              className="nw-editor-body"
              value={body}
              onChange={(event) => handleBodyChange(event.target.value)}
              placeholder="Write your note here."
              dir="auto"
              spellCheck={false}
              aria-label="Note text"
              disabled={loadingNote}
            />
            <div className="nw-editor-actions">
              <button
                type="button"
                className="nw-button-primary"
                onClick={() => void handleSaveNow()}
                disabled={working || loadingNote}
              >
                <Save size={14} aria-hidden="true" />
                {saveWorking ? "Saving..." : "Save now"}
              </button>
              <span className="nw-editor-spacer" />
              <div
                className="nw-toolbar-group"
                role="group"
                aria-label="Editing"
              >
                <button
                  type="button"
                  className="nw-toolbar-btn"
                  onClick={handleUndo}
                  disabled={historyIndex === 0 || working || loadingNote}
                  title="Undo"
                  aria-label="Undo"
                >
                  <Undo2 size={16} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="nw-toolbar-btn"
                  onClick={handleRedo}
                  disabled={
                    historyIndex >= history.length - 1 ||
                    working ||
                    loadingNote
                  }
                  title="Redo"
                  aria-label="Redo"
                >
                  <Redo2 size={16} aria-hidden="true" />
                </button>
                <span className="nw-popup-anchor">
                  <button
                    type="button"
                    className="nw-toolbar-btn"
                    onClick={() => void handleCopy()}
                    disabled={!canCopy || working || loadingNote}
                    title="Copy note text"
                    aria-label="Copy note text"
                  >
                    <Copy size={16} aria-hidden="true" />
                  </button>
                  {copiedUntil !== null && (
                    <span
                      className="nw-popup"
                      role="status"
                      aria-live="polite"
                    >
                      Copied.
                    </span>
                  )}
                </span>
              </div>
              <div
                className="nw-export"
                ref={exportRef}
                onKeyDown={(event) => {
                  if (event.key === "Escape" && exportOpen) {
                    setExportOpen(false);
                  }
                }}
              >
                <button
                  type="button"
                  className="nw-button-secondary"
                  onClick={() => setExportOpen((open) => !open)}
                  disabled={selectedId === null || working || loadingNote}
                  aria-expanded={exportOpen}
                >
                  <Download size={14} aria-hidden="true" />
                  Export
                </button>
                {exportOpen && (
                  <div className="nw-export-menu">
                    <button
                      type="button"
                      className="nw-export-option"
                      onClick={() => void handleExport("txt")}
                    >
                      Text (.txt)
                    </button>
                    <button
                      type="button"
                      className="nw-export-option"
                      onClick={() => void handleExport("md")}
                    >
                      Markdown (.md)
                    </button>
                    <button
                      type="button"
                      className="nw-export-option"
                      onClick={() => void handleExport("erxnote")}
                    >
                      Encrypted (.erxnote)
                    </button>
                  </div>
                )}
                {exportMessage !== null && (
                  <span className="nw-popup" role="status" aria-live="polite">
                    {exportMessage}
                  </span>
                )}
              </div>
              <button
                type="button"
                className="nw-button-danger"
                onClick={() => void handleDelete()}
                disabled={working || loadingNote}
              >
                <Trash2 size={14} aria-hidden="true" />
                Delete
              </button>
            </div>
          </>
        )}
        {error !== null && (
          <p className="nw-error" role="alert">
            {error}
          </p>
        )}
      </section>
    </div>
  );
}
