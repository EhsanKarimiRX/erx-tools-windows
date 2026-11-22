# NEXT - ideas for later versions

This file is a list of ideas. They are not promises and they have no dates. When an idea is built, it moves to the release notes and is removed from this list.

## Secure Notes (after v0.5.0)

- **Text size.** Let the user change the text size in the notes editor.
- **Checkboxes.** Check lists with checkboxes. The Vazirmatn font has no ☐ or ☑ characters, so the boxes must be drawn with CSS or SVG.
- **Text formatting.** Bold, italic, small headings and lists. The design of v0.5.0 (a "format" field in every note and a version number in the file) is meant to leave room for this.
- **Pin notes.** Keep chosen notes at the top of the list.
- **Windows Hello.** Unlock Secure Notes with Windows Hello (TPM). Left out of v0.5.0 on purpose.

## Home screen

- **Move tiles.** Let the user change the order of the tiles on the home screen.

## Quantum Resistance Vault

- **Safer clipboard for the words.** Today the Vault copy button uses the clipboard of the web engine, and the words are removed from the clipboard after 30 seconds. Windows clipboard history may keep a copy anyway (this has not been tested yet). Secure Notes is planned to copy through Rust, with the Windows clipboard history and cloud clipboard switched off. If that works well in tests, the Vault can use the same code. The Vault is not changed in v0.5.0 (owner decision).
