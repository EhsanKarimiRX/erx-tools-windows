# Prompt 6 — HEIC input: decisions

1. **Approach: decode HEIC/HEIF through Windows' own WIC codec** (the user's installed Microsoft HEIF + HEVC extensions). We ship NO decoder. Reason: no LGPL/AGPL obligations, no HEVC patent exposure for us, zero size impact, fully offline.
2. **Rejected:** bundling libheif/libde265; the pure-Rust `heic` crate (AGPL); Media Foundation. Do not use them.
3. **Scope:** HEIC/HEIF **input only**. Outputs stay JPEG, PNG, WebP, BMP, TIFF. Only the **first (primary) image** of a file is converted (same convention as animated GIF/WebP today). Depth maps, gain maps, Live Photo video: ignored.
4. **No color management in v1.** Output pixels are treated as sRGB; ICC is dropped as for all other formats. iPhone photos are usually Display P3, so colors may look slightly less vivid. This is stated honestly in the UI (6d). Do NOT implement ICC/color transforms in this prompt.
5. **10-bit HEIC is reduced to 8-bit** (the pipeline is RGB8/RGBA8). Accepted for v1.
6. **COM binding crate:** `windows-sys` normally provides only raw function declarations and opaque COM pointers, with NO callable interface methods. The investigation assumed only a feature flag was needed — that is probably wrong. Pre-approved by the owner: add the official **`windows`** crate with `default-features = false` and ONLY the features required for WIC/COM (expected: `Win32_Graphics_Imaging`, `Win32_System_Com`, `Win32_Foundation`; add others only if the compiler demands them). **Hand-writing COM vtables is forbidden** (too error-prone). Step 6a-2 verifies this first.
7. **Missing codecs are an expected situation, not a bug.** The app must say so honestly (wording in 6b-6 and 6d).
