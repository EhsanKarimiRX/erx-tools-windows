// Third-party license notices for ERX Tools (Prompt 10b).
//
// Part 1 (always): computes the JavaScript packages that ship in the app —
//   package.json "dependencies" and their transitive production dependencies
//   (dev tools like vite/typescript and type-only packages are NOT included) —
//   and prints a Markdown section with each package, its license, and the
//   license text found in the package folder.
// Part 2 (with --rust FILE): assembles the final THIRD_PARTY_LICENSES.md from
//   a `cargo about generate about.hbs` output file (Rust crates section) plus
//   the JavaScript section, writes it to the repo root and copies it into
//   release-artifacts/v<version>/ when that folder exists.
//
// Usage:
//   node scripts/gen-third-party-js.mjs                 (print JS section)
//   node scripts/gen-third-party-js.mjs --rust FILE     (assemble final file)
import { readFileSync, existsSync, readdirSync, writeFileSync, copyFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const fail = (msg) => { console.error(`GEN-THIRD-PARTY FAILED: ${msg}`); process.exit(1); };
const rustIdx = process.argv.indexOf("--rust");
const rustFile = rustIdx > 0 ? process.argv[rustIdx + 1] : null;

// License strings that must never appear (owner decision): stop and report.
const FORBIDDEN = /(^|\(|\s|\/)(L?GPL|AGPL|SSPL)|unknown|unlicensed/i;

const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const tauriConf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const nodeModules = join(root, "node_modules");

function findLicenseText(dir) {
  for (const f of readdirSync(dir)) {
    if (/^(licen[cs]e|copying)/i.test(f)) {
      return readFileSync(join(dir, f), "utf8");
    }
  }
  return null;
}

// Walk the production dependency closure (direct deps + their "dependencies").
const packages = new Map(); // name -> info
const queue = Object.keys(pkg.dependencies || {});
while (queue.length) {
  const name = queue.shift();
  if (packages.has(name)) continue;
  const dir = join(nodeModules, ...name.split("/"));
  const manifestPath = join(dir, "package.json");
  if (!existsSync(manifestPath)) fail(`package not found in node_modules: ${name} (looked at ${manifestPath})`);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const license = manifest.license || "(license field missing)";
  if (FORBIDDEN.test(license) && !/^(MIT|ISC|BSD|Apache|0BSD|CC0|Zlib|Unlicense|BlueOak)/i.test(license)) {
    fail(`package ${name} has a non-permissive license: "${license}" — per the owner's rule, STOP and report.`);
  }
  packages.set(name, {
    name: manifest.name || name,
    version: manifest.version,
    license,
    homepage: manifest.homepage || (manifest.repository && (manifest.repository.url || manifest.repository)) || "",
    text: findLicenseText(dir),
  });
  for (const d of Object.keys(manifest.dependencies || {})) queue.push(d);
}

const jsList = [...packages.values()].sort((a, b) => a.name.localeCompare(b.name));
let jsSection = `# JavaScript packages

These npm packages are bundled into the app (production dependencies of ERX Tools):

`;
for (const p of jsList) {
  jsSection += `### ${p.name} ${p.version}

License: ${p.license}${p.homepage ? `\nHome page: ${p.homepage}` : ""}

`;
  if (p.text) {
    jsSection += `\`\`\`
${p.text.trimEnd()}
\`\`\`

`;
  } else {
    jsSection += `The license text is not distributed inside this npm package; see the package's home page above for the full text.

`;
  }
}

if (!rustFile) {
  console.log(jsSection);
  process.exit(0);
}

// ---- Part 2: assemble the final THIRD_PARTY_LICENSES.md ----
const rustSection = readFileSync(rustFile, "utf8").trimEnd();
const header = `# Third-party licenses for ERX Tools for Windows

ERX Tools for Windows is MIT-licensed. It includes the following third-party components, each under its own license. Generated for version ${tauriConf.version}.

Windows components such as WebView2 and the HEIF/HEVC codecs are provided by Microsoft and are not part of this file.

`;

const out = `${header}# Rust crates

${rustSection}

${jsSection}`;

const outPath = join(root, "THIRD_PARTY_LICENSES.md");
writeFileSync(outPath, out);
console.log(`wrote ${outPath} (${out.length} bytes, ${jsList.length} JS packages)`);

const relDir = join(root, "release-artifacts", `v${tauriConf.version}`);
if (existsSync(relDir)) {
  copyFileSync(outPath, join(relDir, "THIRD_PARTY_LICENSES.md"));
  console.log(`copied to ${join(relDir, "THIRD_PARTY_LICENSES.md")}`);
} else {
  console.log(`(release-artifacts/v${tauriConf.version} does not exist yet — not copied)`);
}
