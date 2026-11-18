# Metadata Remover

This document describes what the Metadata Remover of ERX Tools actually does. The
source code in `src-tauri/src/tools/metadata_remover/` is the final truth:

| File | Role |
|---|---|
| `dispatch.rs` | Detects the file type from its first bytes and runs the matching handler. |
| `jpeg.rs`, `png.rs`, `webp.rs`, `gif.rs`, `bmp.rs`, `tiff.rs`, `heic.rs`, `mp4.rs`, `mkv.rs` | One handler per format: finds hidden data and builds the cleaning plan. |
| `found.rs` | Turns the raw EXIF and XMP of a file into the readable "found" list. |
| `model.rs` | Shared types: findings, kept notes, the plan (copy / insert / zeros). |
| `patch.rs` | Replaces byte ranges in place (videos, HEIC, TIFF). |
| `executor.rs` | Writes a plan to a file as a stream. |
| `pipeline.rs` | One file end to end: temporary file, checks, final name. |
| `safe_write.rs` | Temporary names, the no-overwrite move, output names. |
| `commands.rs` | The Tauri commands, the batch, progress and cancel. |

Nothing here has been audited by a security professional, and the tool is no
guarantee of anonymity (section 11).

## 1. Overview

For every file the tool first **scans** it (reading headers only, never changing
anything) and shows what hidden data it holds. Cleaning then writes a **new**
file next to the original (or into a chosen folder); the original is only read,
never modified or deleted.

Cleaning never re-encodes pixels or video, so quality cannot change:

- Photos (JPEG, PNG, WebP, GIF) lose their metadata segments and chunks.
- Videos (MP4, MOV, MKV), HEIC and TIFF are changed **in place**: the hidden
  bytes are overwritten with "free" boxes, "Void" elements or zeros, and the
  file keeps its exact size, so every offset stays valid and the picture data is
  never touched.

Scanning and cleaning use the same code: the handler returns
`(findings, kept, plan)`, and the plan is what the user is shown. What the user
sees in the "found" list is exactly what the plan removes.

## 2. Detection: content, never the extension

The first 64 bytes decide the format (a file name or extension is never used):

| Signature | Detected as |
|---|---|
| `FF D8 FF` | JPEG |
| `89 50 4E 47 0D 0A 1A 0A` | PNG |
| `RIFF` + `WEBP` at offset 8 | WebP |
| `GIF87a` / `GIF89a` | GIF |
| `II 2A 00` / `MM 00 2A` | TIFF (classic) |
| `II 2B 00` / `MM 00 2B` | BigTIFF: refused ("BigTIFF files are not supported") |
| `BM` | BMP |
| `1A 45 DF A3` (EBML) | MKV / Matroska (section 5.9) |
| `ftyp` at offset 4, brand at offset 8 | see the brand table below |

Brands in an ISO base media `ftyp` box:

| Brand | Detected as |
|---|---|
| `qt  ` | MOV |
| `heic`, `heix`, `hevc`, `hevx`, `heim`, `heis`, `mif1`, `msf1` | HEIC |
| `avif`, `avis` | AVIF: refused ("AVIF files are not supported") |
| `M4A `, `M4B `, `M4P ` | Audio: refused ("Audio files are not supported") |
| any other brand | MP4 |

Anything else is refused with "not a supported photo or video file". An empty
file gives "file is empty"; a folder gives "folders are not supported". Audio,
documents and PDF files are not supported and are not special-cased: they fall
into the "not a supported photo or video file" answer.

## 3. Size limits

| Kind | Limit |
|---|---|
| Any photo (JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC) | 256 MB |
| Any video (MP4, MOV, MKV) | 16 GB |

A larger file is refused before the handler runs ("file is larger than the 256 MB
limit" / "16 GB limit"). Videos and containers are streamed, so memory stays
small; the pixel check of one big photo is the main memory user.

## 4. What is removed and kept, per format

The labels below are the strings the UI shows in the "found" list.

### 4.1 JPEG

Removed: the EXIF block (with thumbnail, maker notes, GPS), XMP, Photoshop and
IPTC data, comments, the JFIF thumbnail, every other `APPn` segment
(metropolitan MPF, extra application data), and any data after the end-of-image
marker. Kept: the JFIF header (without its thumbnail), the Adobe colour
transform, the structural segments (frame header, Huffman and quantization
tables) and, by option, the ICC colour profile and a fresh minimal EXIF block
holding only the orientation.

### 4.2 PNG

Removed: text chunks (`tEXt`, `zTXt`, `iTXt`, which covers XMP), the
last-modified time (`tIME`), the `eXIf` block, Content Credentials (`caBX`),
every other unknown ancillary chunk and any data after `IEND`. Kept: everything
that affects how the picture is drawn (gamma, chromaticities, sRGB, pixel size,
palette, transparency, background, animation chunks), and by option the ICC
profile (`iCCP`) and a minimal orientation chunk. An unknown **critical** chunk
is refused as an unusual structure.

### 4.3 WebP

Removed: the EXIF chunk, XMP, every unknown chunk and data after the RIFF
container. Kept: the image chunks (VP8, VP8L, ALPH, ANIM, ANMF) and, by option,
the ICC profile (`ICCP`) and a fresh minimal EXIF chunk with only the
orientation. The VP8X flags (which announce metadata chunks) and the RIFF size
are rewritten to match.

### 4.4 GIF

Removed: comment extensions, XMP, every other application extension, unknown
extensions and data after the trailer. Kept: the header, colour tables, frames,
graphic control blocks (timing and transparency), text drawn on the picture,
the animation loop setting (NETSCAPE / ANIMEXTS) and, by option, the ICC
profile.

### 4.5 BMP

A BMP has no metadata fields worth removing, so the handler only confirms the
file really is a BMP and returns an unchanged plan. The file is reported as
"This file has no metadata." and no new file is made.

### 4.6 TIFF (classic, one picture)

Removed: the document name, page name, image description, camera make and
model, software, date, artist, copyright, host computer, XMP, IPTC, Photoshop
data, the whole Exif directory, the whole GPS directory (and a nested
Interop directory), and by option the ICC profile.

The directory (IFD) of the picture is rewritten with only the entries that stay,
and the freed space at its end is zeroed. The data the removed entries pointed
to is zeroed. The picture data and the data of kept entries are never touched.

Kept: every tag that describes the picture, the orientation tag (always), every
unknown tag (GeoTIFF georeferencing is content and stays) and, by option, the
ICC profile. The "keep rotation" option has no effect on TIFF.

Refused: files with more than one page ("multi-page TIFF files are not
supported"), RAW/DNG files (a DNG version tag, "RAW and DNG files are not
supported"), files with sub-pictures, files without strips or tiles, and files
where removed data would overlap the picture data or a kept entry ("unusual
TIFF structure"). Data appended after the last structure of the file is not
detected. BigTIFF is not supported.

### 4.7 HEIC / HEIF

Removed in place: the Exif item and the XMP item (a `mime` item of type
`application/rdf+xml`). Their data is overwritten with zeros and the item type
in its `infe` entry is changed to `free`, so no reader mistakes the zeros for
metadata. Pictures, tiles, thumbnails and the HDR gain map are never touched.

Kept: the picture data, the rotation (a HEIC photo stores its rotation with the
picture in `irot` / `imir`, not in Exif, so removing Exif does not turn the
photo; the "keep rotation" option has no effect on HEIC) and the colour profile
(a property of the picture, it cannot be removed; the UI says so).

Refused: a file with no `meta` box ("no picture information found in the
file"), a file with several `meta` boxes, metadata whose data does not lie
inside `mdat` (or `idat`), metadata that overlaps a picture item, unsupported
`iloc` versions or field sizes ("unusual HEIC structure").

### 4.8 MP4 and MOV

Nothing is re-encoded or moved. Metadata boxes are overwritten **in place** with
`free` boxes of the same size, so offsets stay valid and the file keeps its size:

- `udta` (user data), `meta` (location, make, model, software, creation date
  keys), `uuid` boxes (XMP and others), and top-level preview pictures
  (`thum`, `PICT`, `pnot`);
- **timed-metadata tracks** (a track whose handler is `meta`: location, motion
  or camera data) are replaced by a `free` box **and their samples inside
  `mdat` are overwritten with zeros**, located through the track's own sample
  tables (`stsz`, `stsc`, `stco` / `co64`);
- the creation and modification times of the movie, tracks and media
  (`mvhd`, `tkhd`, `mdhd`) are set to zero.

Kept: video, audio and subtitle tracks, untouched. In a fragmented file (`moof`
or `mvex`) the tracks are left in place and the UI says so.

Refused: a file without `moov` ("no video information found in the file"),
impossible box sizes ("damaged or incomplete video file"), and timed-metadata
samples that do not lie inside an `mdat` box ("unusual video structure").

### 4.9 MKV (Matroska)

Removed in place with `Void` elements of the same total size (every player
skips them): the `Tags` element (statistics, encoder and other extra
information), attached **pictures** (cover art), the title, the date and the
file names stored in the segment info, and a CRC-32 element inside an element
that changed (its checksum would no longer match). The muxing and writing
program names are required elements, so their text is overwritten with zeros
instead of removing the element.

Kept: all video, audio and subtitle data, attached **fonts** (styled subtitles
need them) and chapters (they are content).

Refused: a file that does not start with the EBML header ("not a valid MKV
file"), a file without a Segment, and an element of unknown size inside the
Segment (an unfinished recording, "unusual video structure"). A Segment of
unknown size itself is accepted.

Note: detection is by the EBML signature, not by the DocType, so a WebM file is
handled by this same MKV code path. WebM is not in the advertised format list
and was never tested with a sample.

## 5. Options

| Option | Default | Effect |
|---|---|---|
| Keep photo rotation | on | JPEG and WebP get a fresh minimal EXIF block with only the orientation; PNG gets a fresh `eXIf` chunk. TIFF always keeps its orientation tag; HEIC keeps its rotation inside the picture. |
| Keep colour profile | on | JPEG `APP2` ICC, PNG `iCCP`, WebP `ICCP`, GIF ICC and the TIFF `ICCProfile` tag are kept. Turning it off removes them, and colours may then look different. HEIC cannot lose its profile. |
| Output location | same folder | Each new file goes next to its original, or all of them into one chosen folder (an absolute path that must exist). |

Changing either "keep" option scans the files in the list again, because it
changes what is found.

## 6. Output files

- Name: the original stem + `ERX` + the original extension (`photo.jpg` becomes
  `photoERX.jpg`). A taken name counts up: `photoERX (1).jpg`,
  `photoERX (2).jpg`, and so on.
- A file with nothing to remove is skipped: no new file is made and the UI says
  "This file has no metadata."
- The original file is never modified or deleted by the tool. The new file gets
  the current file-system dates (it is a new file).
- A duplicate file (the same path, compared without case) is not added twice to
  the list.

## 7. Safe writing

1. The cleaned copy is written to `<final name>.<8 random hex>.erxtmp` in the
   output folder, as a stream.
2. It is flushed and synced to disk.
3. It is moved to its final name with `MoveFileExW` **without**
   `MOVEFILE_REPLACE_EXISTING`, so Windows itself refuses an existing target,
   atomically. `std::fs::rename` is not used because it replaces an existing
   file. A name that got taken in the meantime counts up and is retried; a
   short lock by antivirus is retried up to 5 times, 150 ms apart.
4. A failed check or a cancel removes the temporary file. Temporary files older
   than one hour are removed automatically when the next file is cleaned in the
   same folder.

## 8. Verification before the final name

A cleaned copy is only renamed to its final name after three checks:

1. the written size equals the size the plan describes;
2. the new file is scanned again by the same handler and must have nothing left
   to remove;
3. for photos, the pixels are compared with the original: RGBA pixels decoded by
   the `image` crate and hashed with SHA-256, one picture in memory at a time.
   For HEIC the picture is decoded by Windows (WIC, through the Image
   Converter's HEVC decoder) into a PNG in memory and that PNG is hashed.

The pixel comparison is skipped, and the report says so, for videos, for HEIC
when Windows cannot decode the original (for example the HEIF codecs are
missing), for pictures the decoder cannot read, and for pictures over 80
megapixels. If the new file itself cannot be read back, the file is reported as
failed. A failed check removes the temporary file and reports the file as
failed; the original is untouched.

## 9. Batches, progress and cancel

- At most 200 files in one list (enforced in the UI and again in Rust).
- At most one batch at a time; a second start is refused.
- At most two files are cleaned at the same time (the work is limited by the
  disk, and the pixel check of a big photo needs a lot of memory).
- Every status change arrives in the UI as a progress event: `working` (bytes
  done and total, at most one event per file every 100 ms), `done`, `nothing`,
  `failed` or `skipped`, each with the output path and sizes, what was removed
  and kept, and whether the pixels were compared.
- Cancel stops the files that are being cleaned (their temporary files are
  removed) and marks the rest as skipped. A failing file never stops the batch.
- The batch ends with four numbers: cleaned, already clean, failed, cancelled.

## 10. Limits and refused files (summary)

| Situation | Result |
|---|---|
| Empty file | "file is empty" |
| Folder | "folders are not supported" |
| Unknown content, audio, document, PDF | "not a supported photo or video file" |
| AVIF / AVIS | "AVIF files are not supported" |
| Audio in an MP4 container (M4A, M4B, M4P) | "Audio files are not supported" |
| BigTIFF | "BigTIFF files are not supported" |
| Photo over 256 MB, video over 16 GB | "file is larger than the ... limit" |
| More than 200 files in one list | "At most 200 files can be added at once." |
| Multi-page TIFF, RAW/DNG, TIFF with sub-pictures or without strips | as listed in section 4.6 |
| Damaged or unusual structure | "damaged or incomplete ... file" or "unusual ... structure" |
| Cleaning already running | "Another cleaning is already running." |
| Output folder not absolute or missing | "The output folder must be an absolute path." / "The chosen output folder does not exist." |

Internal caps that exist so a hostile file cannot exhaust memory: 20,000 JPEG
segments, 1,000,000 PNG chunks, 100,000 WebP chunks, 1,000,000 GIF blocks,
512 TIFF directory entries, 200,000 MP4 boxes, 1,000,000 MKV elements.

## 11. What the tool cannot do

Only metadata fields are removed. The tool cannot remove:

- what is visible in the picture or video (including watermarks burned into the
  picture);
- the file name, the dates of the new file, or the fact that a new file exists;
- traces inside the compressed picture or video stream itself (for example an
  encoder or application name written into the HEVC/H.264 bitstream, or a
  thumbnail that some formats store inside the picture data);
- other copies of the file (backups, cloud sync, the original itself until the
  user deletes it);
- data appended after the last structure of a TIFF file (not detected);
- C2PA Content Credentials everywhere: only the PNG `caBX` chunk is recognized
  and removed. In MP4/MOV a `uuid` box is removed as generic extra data, but an
  item or box the handler does not know (for example inside a HEIC file) stays.

The tool is not a guarantee of anonymity.

## 12. Test status

- Platform: Windows 11 x64 only. Windows 10 and ARM64 were not tested (ARM64 was
  only compiled).
- Automated tests: the Rust unit tests of `metadata_remover` (synthetic files for
  every format, every truncation of a sample, 3,000 random byte flips per
  format), plus ignored "live" tests that use the owner's samples in
  `C:/dev/erxtools-testdata/Mediatest`.
- Live samples used during development: `A.jpg`, `A.HEIC` (a JPEG with the wrong
  extension), `A.png`, `A.MOV`, `A.mp4`, `A.mkv`, `b.heic` to `f.heic` (iPhone
  grid photos, a Samsung-style two-item file and a single-item file) and
  `file_example_TIFF_5MB.tiff`.
- The owner tested the samples and some unsupported files by hand: detection,
  cleaning, the original staying untouched, cancel, the output folder and
  opening the cleaned files all worked as expected.
- WebM, M4V, AVIF, audio, Windows 10 and ARM64 were not tested with files.
