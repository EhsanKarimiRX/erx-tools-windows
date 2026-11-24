/** The notes screen shell. N8b fills the list and the editor; for now it only
 * shows the two-column frame so the window layout is real. */
export default function NotesScreen() {
  return (
    <div className="nw-notes">
      <section className="nw-list-pane" aria-label="Notes list">
        <h2 className="nw-pane-title">Notes</h2>
        <p className="nw-muted">Your notes will appear here.</p>
      </section>
      <section className="nw-editor-pane" aria-label="Note editor">
        <p className="nw-muted">Select or create a note.</p>
      </section>
    </div>
  );
}
