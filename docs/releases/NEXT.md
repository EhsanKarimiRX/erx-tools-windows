**ERX Tools for Windows — next release (draft)**

## Image Converter

- Output files now always get an "ERX" suffix before the extension: `photo.jpg` becomes `photoERX.png`, `201.HEIC` becomes `201ERX.jpg` (same-format conversions too, instead of the old `-converted` name). If that name is taken it counts up: `photoERX (1).png`, `photoERX (2).png`, …
- Converted files are never overwritten — now guaranteed even when several files with the same name (for example from different folders) are converted in one batch, and even if another program creates a file at the same moment.
- Settings panel: more space under the "Output location" title, and the settings notes are now justified.

## QR Code Generator

- The logo is now exactly centered: the cleared area behind it lines up with the logo for any quiet-zone margin.
- The export size is a typed number (256–4096 px, digits only, checked when you confirm) and the PNG is saved at exactly that size — no more rounding down. The default is now 1080 × 1080.
- In the Rounded style, the cleared area behind the logo now has rounded corners (a background-colored plate under the logo; skipped on transparent backgrounds).
- Save PNG / Save SVG now open a real Save dialog (confirm button "Save", with the standard overwrite confirmation) and always give the file the right extension.
- While a logo is added, the other error-correction levels are greyed out and explain on click why they are unavailable (H is required).
