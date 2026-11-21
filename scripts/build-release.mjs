// Builds the two Windows installers + SHA256SUMS.txt into release-artifacts/v<version>/.
// Usage: npm run release:build            (real build, takes a long time)
//        node scripts/build-release.mjs --dry-run   (prints what it would do)
import { spawnSync } from "node:child_process";
import { readFileSync, existsSync, mkdirSync, rmSync, copyFileSync, readdirSync, writeFileSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const dryRun = process.argv.includes("--dry-run");
const fail = (msg) => { console.error(`\nRELEASE BUILD FAILED: ${msg}`); process.exit(1); };

const tauriConf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const cargoToml = readFileSync(join(root, "src-tauri", "Cargo.toml"), "utf8");
const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargoToml)?.[1];
const version = tauriConf.version;
if (pkg.version !== version || cargoVersion !== version) {
  fail(`version mismatch: tauri.conf.json=${version}, package.json=${pkg.version}, Cargo.toml=${cargoVersion}`);
}

// The installer must download WebView2 only when the PC does not have it.
const webviewMode = tauriConf.bundle?.windows?.webviewInstallMode?.type;
if (webviewMode !== "downloadBootstrapper") {
  fail(`bundle.windows.webviewInstallMode.type must be "downloadBootstrapper" (found "${webviewMode}"): the installer must download WebView2 only when it is missing and must not embed it`);
}

// A normal installer is about 4 MB; an installer with embedded WebView2 would be over 100 MB.
const MAX_INSTALLER_BYTES = 50 * 1024 * 1024;

const nsisDir = (...parts) => join(root, "src-tauri", "target", ...parts, "release", "bundle", "nsis");
const targets = [
  { arch: "x64", args: [], dir: nsisDir() },
  { arch: "arm64", args: ["--target", "aarch64-pc-windows-msvc"], dir: nsisDir("aarch64-pc-windows-msvc") },
];

const outDir = join(root, "release-artifacts", `v${version}`);
if (!dryRun) mkdirSync(outDir, { recursive: true });
const produced = [];

for (const t of targets) {
  const outName = `ERX-Tools-${version}-${t.arch}-setup.exe`;
  const args = ["run", "tauri", "build", "--", ...t.args];
  console.log(`\n=== ${outName} ===\nnpm ${args.join(" ")}`);
  if (dryRun) { produced.push(outName); continue; }
  // Remove stale installers first so the build's own file is the only one.
  if (existsSync(t.dir)) for (const f of readdirSync(t.dir)) if (f.endsWith("-setup.exe")) rmSync(join(t.dir, f));
  const r = spawnSync("npm", args, { cwd: root, stdio: "inherit", shell: true });
  if (r.status !== 0) fail(`${outName}: tauri build exited with code ${r.status}`);
  const wanted = `_${version}_${t.arch}-setup.exe`;
  const found = existsSync(t.dir) ? readdirSync(t.dir).filter((f) => f.endsWith(wanted)) : [];
  if (found.length !== 1) fail(`${outName}: expected exactly one installer ending with ${wanted} in ${t.dir}, found ${found.length}`);
  copyFileSync(join(t.dir, found[0]), join(outDir, outName));
  const copiedBytes = statSync(join(outDir, outName)).size;
  if (copiedBytes > MAX_INSTALLER_BYTES) {
    fail(`${outName} is ${(copiedBytes / 1048576).toFixed(1)} MB, above the ${MAX_INSTALLER_BYTES / 1048576} MB limit: the installer must not embed WebView2`);
  }
  produced.push(outName);
}

if (!dryRun) {
  const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
  const lines = produced.map((n) => `${sha(join(outDir, n))}  ${n}`);
  writeFileSync(join(outDir, "SHA256SUMS.txt"), lines.join("\n") + "\n");
  console.log("\nFiles:");
  for (const n of produced) console.log(`  ${n}  ${(statSync(join(outDir, n)).size / 1048576).toFixed(1)} MB`);
  console.log(`\nOutput folder: ${outDir}`);
}
console.log("\nRELEASE BUILD DONE");
