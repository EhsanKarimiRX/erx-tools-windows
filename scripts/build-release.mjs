// Builds the four Windows installers + SHA256SUMS.txt into release-artifacts/v<version>/.
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

const nsisDir = (...parts) => join(root, "src-tauri", "target", ...parts, "release", "bundle", "nsis");
const targets = [
  { arch: "x64", args: [], dir: nsisDir() },
  { arch: "arm64", args: ["--target", "aarch64-pc-windows-msvc"], dir: nsisDir("aarch64-pc-windows-msvc") },
];
const variants = [
  { suffix: "", config: join("src-tauri", "release-configs", "webview2-download.json") },
  { suffix: "-with-webview2", config: join("src-tauri", "release-configs", "webview2-offline.json") },
];

const outDir = join(root, "release-artifacts", `v${version}`);
if (!dryRun) mkdirSync(outDir, { recursive: true });
const produced = [];

for (const t of targets) {
  for (const v of variants) {
    const outName = `ERX-Tools-${version}-${t.arch}-setup${v.suffix}.exe`;
    const args = ["run", "tauri", "build", "--", ...t.args, "--config", v.config];
    console.log(`\n=== ${outName} ===\nnpm ${args.join(" ")}`);
    if (dryRun) { produced.push(outName); continue; }
    // Remove stale installers first: both variants of one architecture produce the SAME file name.
    if (existsSync(t.dir)) for (const f of readdirSync(t.dir)) if (f.endsWith("-setup.exe")) rmSync(join(t.dir, f));
    const r = spawnSync("npm", args, { cwd: root, stdio: "inherit", shell: true });
    if (r.status !== 0) fail(`${outName}: tauri build exited with code ${r.status}`);
    const wanted = `_${version}_${t.arch}-setup.exe`;
    const found = existsSync(t.dir) ? readdirSync(t.dir).filter((f) => f.endsWith(wanted)) : [];
    if (found.length !== 1) fail(`${outName}: expected exactly one installer ending with ${wanted} in ${t.dir}, found ${found.length}`);
    copyFileSync(join(t.dir, found[0]), join(outDir, outName));
    produced.push(outName);
  }
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
