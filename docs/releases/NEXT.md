**ERX Tools for Windows — next release (draft)**

## Image Converter

- Output files now always get an "ERX" suffix before the extension: `photo.jpg` becomes `photoERX.png`, `201.HEIC` becomes `201ERX.jpg` (same-format conversions too, instead of the old `-converted` name). If that name is taken it counts up: `photoERX (1).png`, `photoERX (2).png`, …
- Converted files are never overwritten — now guaranteed even when several files with the same name (for example from different folders) are converted in one batch, and even if another program creates a file at the same moment.
- Settings panel: more space under the "Output location" title, and the settings notes are now justified.
