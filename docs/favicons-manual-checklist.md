# Favicons: manual test checklist

Run the app with `npm run tauri dev`. Prepare these test files (for example in C:\dev\erxtools-testdata):

| File | What it is |
|---|---|
| logo.png | transparent square logo, at least 512x512 |
| logo.svg | simple SVG logo without text |
| logo-text.svg | SVG that contains a <text> element |
| photo.jpg | normal JPEG photo |
| wide.png | non-square PNG (for example 800x300) |
| tiny.png | small PNG (for example 64x64) |
| fake.png | a text file renamed to .png |
| really-png.jpg | a PNG file renamed to .jpg |
| (optional) photo.heic | iPhone photo |

## A. Loading the source
- [ ] Open the Favicons tile from the home screen; the empty state shows the drop zone and the explanation text.
- [ ] Drop logo.png on the window: thumbnail, file name, format and size appear, and the preview grid fills with 6 icons.
- [ ] Use "Choose image" instead of dropping: same result.
- [ ] Drop logo.svg: kind SVG, preview works, and the saved package contains favicon.svg.
- [ ] Drop logo-text.svg: warning that text is not rendered.
- [ ] Drop photo.jpg: warning that JPEG has no transparency.
- [ ] Drop wide.png: warning "not square" appears in the preview step.
- [ ] Drop tiny.png: "source is small" warning.
- [ ] Drop fake.png: a red error message, no crash, the previous image (if any) stays usable.
- [ ] Drop really-png.jpg: works like a normal PNG (content detection).
- [ ] Drop photo.heic (if available): friendly HEIC message.
- [ ] Drop two files at once: only the first is used and a note says so.
- [ ] Drop a new image while one is loaded: it replaces the old one and the saved panel disappears.

## B. Options
- [ ] Website name: it changes the saved folder name and the manifest. Try: My Site, a name with quotes, a Persian name, an empty name.
- [ ] Favicon padding slider and number box stay in sync, limited to 0-45. Only the 16x16 and 32x32 previews (enlarged in the grid) change; Apple and Android icons do not change.
- [ ] Home screen icon padding (default 10): changes the Apple touch icon and the Android 192/512 icons only; at 0 the logo touches the edges again. The maskable icon and the small favicons do not change.
- [ ] "Trim empty transparent border": test with a logo that has a big transparent margin.
- [ ] Background color: changes the Apple icon and the maskable icon only.
- [ ] Theme color: changes the theme-color line in the HTML snippet.
- [ ] Invalid color text (for example #12): red hint, Save button disabled, no error from Rust. A valid color re-enables it.
- [ ] "Include maskable icon" off: the maskable icon disappears from the preview and from the snippet/manifest.
- [ ] Moving a slider fast does not flicker or show an old preview (debounce).

## C. Saving
- [ ] "Create favicon package" opens a folder dialog. Cancel it: nothing happens, no error.
- [ ] Save into a folder: a new folder `<name>_favicon_package` appears with these files: favicon.ico, favicon-16x16.png, favicon-32x32.png, apple-touch-icon.png, android-chrome-192x192.png, android-chrome-512x512.png, android-chrome-maskable-512x512.png, site.webmanifest, favicon-snippet.html (+ favicon.svg for SVG sources).
- [ ] Save again into the same folder: a second folder with suffix _1 is created, the first one is untouched.
- [ ] The saved panel lists all files with sizes and the folder path.
- [ ] "Show in folder" opens Explorer with the file selected.
- [ ] Changing any option after saving hides the saved panel.
- [ ] Save into a folder where writing is not allowed (for example C:\Windows\System32): a clear red error, no half-written folder left behind.

## D. Quality of the output files
- [ ] favicon.ico opens in an image viewer and contains 16, 32 and 48 px images.
- [ ] apple-touch-icon.png is 180x180, has NO transparency (solid background color) and the logo does not touch the edges (default padding 10%).
- [ ] The maskable icon is 512x512, solid background, logo inside a safe margin.
- [ ] android-chrome icons keep transparency and have the home screen padding.
- [ ] No dark halo around the logo edges in the small icons (16 and 32).
- [ ] site.webmanifest is valid JSON (open it in a JSON validator) and shows the site name correctly, also for a name with quotes.
- [ ] favicon-snippet.html matches the snippet shown in the app.

## E. Real browser and phone test
- [ ] Put the generated files and a small index.html (with the snippet inside <head>) in one folder, run `python -m http.server` there and open http://localhost:8000 in Chrome/Edge/Firefox: the tab shows the icon.
- [ ] Chrome DevTools -> Application -> Manifest shows the name and the icons without errors.
- [ ] (Optional, real website over HTTPS) iPhone Safari: Share -> Add to Home Screen shows the Apple touch icon with comfortable margins.
- [ ] (Optional) Android Chrome: Add to Home screen / Install app shows the icon correctly (maskable icon not cropped).

## F. UI
- [ ] Light and dark Windows theme both look right (checkerboard visible, text readable).
- [ ] Narrow window (about 700 px): options move below the preview, nothing overflows.
- [ ] Keyboard only: Tab reaches every control with a visible focus ring; the snippet box is focusable.
- [ ] Back button returns to the home screen; opening the tool again starts empty.
- [ ] Large source (for example a 4000x4000 PNG): the app stays responsive while the preview is generated.
