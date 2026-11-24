/** The notes screen: the note list on the left, the editor on the right.
 * Edits are debounced and sent to Rust; the shell owns the save indicator. */
import { useEffect, useRef, useState } from "react";
import {
  copyText,
  createNote,
  deleteNote,
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

interface PendingUpdate {
  id: string;
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

  const searchTimer = useRef<number | null>(null);
  const updateTimer = useRef<number | null>(null);
  const selectedIdRef = useRef<string | null>(null);
  const queryRef = useRef("");
  const pendingRef = useRef<PendingUpdate | null>(null);
  const noteSeq = useRef(0);
  const titleRef = useRef<HTMLInputElement>(null);
  const focusTitle = useRef(false);
  const newNoteRef = useRef<() => void>(() => {});

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

  function handleTitleChange(value: string) {
    setTitle(value);
    onDirty();
    scheduleUpdate(value, body);
  }

  function handleBodyChange(value: string) {
    setBody(value);
    onDirty();
    scheduleUpdate(title, value);
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
    setCopiedUntil(null);
    setError(null);
    setLoadingNote(true);
    try {
      const note = await getNote(id);
      if (seq !== noteSeq.current) return;
      setTitle(note.title);
      setBody(note.body);
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
      setLoadingNote(false);
      setCopiedUntil(null);
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
      setLoadingNote(false);
      setCopiedUntil(null);
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

  return (
    <div className="nw-notes">
      <section className="nw-list-pane" aria-label="Notes list">
        <div className="nw-pane-head">
          <h2 className="nw-pane-title">Notes</h2>
          <button
            type="button"
            className="nw-button-secondary"
            onClick={() => void handleNew()}
            disabled={working}
          >
            New note
          </button>
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
            {notes.map((note) => (
              <li key={note.id}>
                <button
                  type="button"
                  className={
                    note.id === selectedId
                      ? "nw-list-item selected"
                      : "nw-list-item"
                  }
                  onClick={() => void selectNote(note.id)}
                  aria-current={note.id === selectedId ? "true" : undefined}
                >
                  <span className="nw-list-title" dir="auto">
                    {note.title.trim() === "" ? "(untitled)" : note.title}
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
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="nw-editor-pane nw-editor" aria-label="Note editor">
        {selectedId === null ? (
          <p className="nw-empty">Select a note or create a new one.</p>
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
                {saveWorking ? "Saving..." : "Save now"}
              </button>
              <button
                type="button"
                className="nw-button-secondary"
                onClick={() => void handleCopy()}
                disabled={!canCopy || working || loadingNote}
              >
                Copy
              </button>
              {copiedUntil !== null && (
                <span className="nw-copied" role="status">
                  Copied.
                </span>
              )}
              <span className="nw-editor-spacer" />
              <button
                type="button"
                className="nw-button-danger"
                onClick={() => void handleDelete()}
                disabled={working || loadingNote}
              >
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
