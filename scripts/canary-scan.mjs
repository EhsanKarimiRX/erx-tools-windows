// Canary scanner: search folders for unique test strings in file contents
// and in file/folder names. Built with Node built-in modules only.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";

export const MIN_CANARY_LENGTH = 12;
export const DEFAULT_CHUNK_SIZE = 4 * 1024 * 1024;
const DEFAULT_GEN_OUT = "C:\\dev\\erxtools-testdata\\canary";

function jsonEscape(text) {
  let out = "";
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i);
    if (code > 126) {
      out += "\\u" + code.toString(16).padStart(4, "0");
    } else {
      out += text[i];
    }
  }
  return out;
}

export function buildPatterns(canary) {
  if (typeof canary !== "string" || canary.length < MIN_CANARY_LENGTH) {
    throw new Error(`Canary must be at least ${MIN_CANARY_LENGTH} characters long.`);
  }
  const candidates = [
    { name: "utf8", bytes: Buffer.from(canary, "utf8") },
    { name: "utf16le", bytes: Buffer.from(canary, "utf16le") },
    { name: "json-escape", bytes: Buffer.from(jsonEscape(canary), "utf8") },
    { name: "url-encoded", bytes: Buffer.from(encodeURIComponent(canary), "utf8") },
  ];
  const patterns = [];
  for (const candidate of candidates) {
    if (!patterns.some((pattern) => pattern.bytes.equals(candidate.bytes))) {
      patterns.push(candidate);
    }
  }
  return patterns;
}

export function buildAllPatterns(canaries) {
  const all = [];
  canaries.forEach((canary, index) => {
    for (const pattern of buildPatterns(canary)) {
      all.push({ name: pattern.name, bytes: pattern.bytes, canary: index + 1 });
    }
  });
  return all;
}

export function nameNeedles(canary) {
  const needles = [canary];
  const encoded = encodeURIComponent(canary);
  if (!needles.includes(encoded)) {
    needles.push(encoded);
  }
  return needles;
}

export function scanFile(filePath, patterns, options = {}) {
  const chunkSize = options.chunkSize ?? DEFAULT_CHUNK_SIZE;
  const fileSystem = options.fs ?? fs;
  const maxLen = patterns.reduce((max, pattern) => Math.max(max, pattern.bytes.length), 0);
  const pending = patterns.map((pattern) => ({ ...pattern, found: false }));
  const hits = [];
  let bytes = 0;
  let carry = Buffer.alloc(0);
  let fd;
  try {
    fd = fileSystem.openSync(filePath, "r");
    const buffer = Buffer.alloc(chunkSize);
    let read;
    while ((read = fileSystem.readSync(fd, buffer, 0, chunkSize, null)) > 0) {
      const chunk = buffer.subarray(0, read);
      const windowStart = bytes - carry.length;
      const window = Buffer.concat([carry, chunk]);
      for (const pattern of pending) {
        if (pattern.found) {
          continue;
        }
        const index = window.indexOf(pattern.bytes);
        if (index >= 0) {
          pattern.found = true;
          const hit = { pattern: pattern.name, offset: windowStart + index, canary: pattern.canary };
          hits.push(hit);
        }
      }
      bytes += read;
      if (maxLen > 1) {
        const keep = Math.min(maxLen - 1, window.length);
        carry = window.subarray(window.length - keep);
      } else {
        carry = Buffer.alloc(0);
      }
    }
  } finally {
    if (fd !== undefined) {
      fileSystem.closeSync(fd);
    }
  }
  return { hits, bytes };
}

function buildNeedleCanaryMap(patterns, needles) {
  const canaryTexts = new Map();
  for (const pattern of patterns) {
    if (pattern.name === "utf8" && pattern.canary !== undefined) {
      canaryTexts.set(pattern.canary, pattern.bytes.toString("utf8"));
    }
  }
  const map = new Map();
  for (const needle of needles) {
    if (map.has(needle)) {
      continue;
    }
    for (const [canary, text] of canaryTexts) {
      if (text.includes(needle) || encodeURIComponent(text).includes(needle)) {
        map.set(needle, canary);
        break;
      }
    }
  }
  return map;
}

export function scanFolders(folders, patterns, needles, options = {}) {
  const fileSystem = options.fs ?? fs;
  const chunkSize = options.chunkSize ?? DEFAULT_CHUNK_SIZE;
  const onHit = options.onHit;
  const result = {
    hits: [],
    filesScanned: 0,
    bytesScanned: 0,
    unreadable: [],
    skippedLinks: [],
    foldersMissing: [],
  };
  const needleCanary = buildNeedleCanaryMap(patterns, needles);

  for (const folder of folders) {
    const stack = [folder];
    while (stack.length > 0) {
      const dir = stack.pop();
      let entries;
      try {
        entries = fileSystem.readdirSync(dir, { withFileTypes: true });
      } catch (err) {
        if (dir === folder && err.code === "ENOENT") {
          result.foldersMissing.push(dir);
        } else {
          result.unreadable.push({ path: dir, code: err.code });
        }
        continue;
      }
      for (const entry of entries) {
        const entryPath = path.join(dir, entry.name);
        const matchedNeedle = needles.find((needle) => entry.name.includes(needle));
        if (matchedNeedle !== undefined) {
          const hit = {
            path: entryPath,
            kind: "name",
            pattern: "name",
            offset: null,
            canary: needleCanary.get(matchedNeedle) ?? null,
          };
          result.hits.push(hit);
          if (onHit) {
            onHit(hit);
          }
        }
        if (entry.isSymbolicLink()) {
          result.skippedLinks.push(entryPath);
          continue;
        }
        if (entry.isDirectory()) {
          stack.push(entryPath);
          continue;
        }
        if (entry.isFile()) {
          let scan;
          try {
            scan = scanFile(entryPath, patterns, { fs: fileSystem, chunkSize });
          } catch (err) {
            result.unreadable.push({ path: entryPath, code: err.code });
            continue;
          }
          result.filesScanned += 1;
          result.bytesScanned += scan.bytes;
          for (const contentHit of scan.hits) {
            const hit = {
              path: entryPath,
              kind: "content",
              pattern: contentHit.pattern,
              offset: contentHit.offset,
              canary: contentHit.canary,
            };
            result.hits.push(hit);
            if (onHit) {
              onHit(hit);
            }
          }
        }
      }
    }
  }
  return result;
}

export function exitCodeFor(result) {
  if (result.hits.length > 0) {
    return 1;
  }
  if (result.unreadable.length > 0) {
    return 3;
  }
  return 0;
}

export function parseCanaryFile(text) {
  let content = text;
  if (content.charCodeAt(0) === 0xfeff) {
    content = content.slice(1);
  }
  return content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

export function generateCanaries(token) {
  const SALAM = "\u0633\u0644\u0627\u0645";
  const MIKHAHAM = "\u0645\u06CC\u200C\u062E\u0648\u0627\u0647\u0645";
  const DIGITS1 = "\u06F1\u06F2\u06F3";
  const DIGITS2 = "\u06F4\u06F5\u06F6";
  return [
    `ERXCANARY-${token}-ENGLISH-hello-world`,
    `ERXCANARY-${token}-FARSI-${SALAM}-${MIKHAHAM}-${DIGITS1}`,
    `ERXCANARY-${token}-MIXED-hello-${SALAM}-world-${DIGITS2}`,
  ];
}

const USAGE = [
  "Usage:",
  "  node scripts/canary-scan.mjs gen [--out <folder>]",
  "  node scripts/canary-scan.mjs scan [--canary-file <file>] [--canary <text>]... [--folder <path>]... [--default-folders]",
].join("\n");

function usage(message) {
  if (message) {
    console.error(`ERROR: ${message}`);
  }
  console.error(USAGE);
  return 2;
}

function runGen(args) {
  let out = DEFAULT_GEN_OUT;
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--out") {
      const value = args[i + 1];
      if (!value) {
        return usage("--out needs a folder.");
      }
      out = value;
      i += 1;
    } else {
      return usage(`Unknown option: ${arg}`);
    }
  }
  fs.mkdirSync(out, { recursive: true });
  const token = crypto.randomBytes(4).toString("hex");
  const canaries = generateCanaries(token);
  const file = path.join(out, `canary-${token}.txt`);
  const content =
    [
      "# ERX canary strings. Open this file in Notepad.",
      "# Line 1: type it by hand. Line 2: paste it. Line 3: type or paste it.",
      ...canaries,
    ].join("\n") + "\n";
  fs.writeFileSync(file, content, "utf8");
  console.log(file);
  console.log("Open it in Notepad.");
  console.log(`node scripts\\canary-scan.mjs scan --canary-file "${file}" --default-folders`);
  return 0;
}

function runScan(args) {
  const canaryValues = [];
  const folders = [];
  let canaryFile = null;
  let useDefaultFolders = false;

  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--canary-file") {
      const value = args[i + 1];
      if (!value) {
        return usage("--canary-file needs a file.");
      }
      canaryFile = value;
      i += 1;
    } else if (arg === "--canary") {
      const value = args[i + 1];
      if (!value) {
        return usage("--canary needs a value.");
      }
      canaryValues.push(value);
      i += 1;
    } else if (arg === "--folder") {
      const value = args[i + 1];
      if (!value) {
        return usage("--folder needs a path.");
      }
      folders.push(value);
      i += 1;
    } else if (arg === "--default-folders") {
      useDefaultFolders = true;
    } else {
      return usage(`Unknown option: ${arg}`);
    }
  }

  const canaries = [];
  if (canaryFile) {
    let text;
    try {
      text = fs.readFileSync(canaryFile, "utf8");
    } catch (err) {
      return usage(`Cannot read canary file: ${canaryFile} (${err.code ?? err.message})`);
    }
    canaries.push(...parseCanaryFile(text));
  }
  canaries.push(...canaryValues);

  if (canaries.length === 0) {
    return usage("No canary given.");
  }
  for (const canary of canaries) {
    if (canary.length < MIN_CANARY_LENGTH) {
      return usage(`Canary is shorter than ${MIN_CANARY_LENGTH} characters.`);
    }
  }

  if (useDefaultFolders) {
    const localAppData = process.env.LOCALAPPDATA;
    const appData = process.env.APPDATA;
    if (!localAppData || !appData) {
      console.error("ERROR: LOCALAPPDATA and APPDATA must both be set for --default-folders.");
      return 2;
    }
    folders.push(
      path.join(localAppData, "com.erxtools.windows"),
      path.join(appData, "com.erxtools.windows"),
      os.tmpdir(),
      path.join(localAppData, "ERX Tools"),
      path.join(localAppData, "CrashDumps"),
      path.join(localAppData, "Microsoft", "Windows", "WER"),
    );
  }

  if (folders.length === 0) {
    return usage("No folder given.");
  }

  const patterns = buildAllPatterns(canaries);
  const needles = [...new Set(canaries.flatMap((canary) => nameNeedles(canary)))];

  console.log("ERX canary scan");
  for (const folder of folders) {
    console.log(`${fs.existsSync(folder) ? "(found)" : "(missing)"} ${folder}`);
  }

  const result = scanFolders(folders, patterns, needles, {
    onHit: (hit) => {
      const encoding = hit.kind === "name" ? "name" : hit.pattern;
      const offset = hit.offset === null || hit.offset === undefined ? "-" : String(hit.offset);
      console.log(`HIT ${hit.kind} ${encoding} canary#${hit.canary} offset=${offset} ${hit.path}`);
    },
  });

  const scannedFolders = folders.length - result.foldersMissing.length;
  const megabytes = (result.bytesScanned / (1024 * 1024)).toFixed(2);
  console.log(`Folders scanned: ${scannedFolders}`);
  console.log(`Files scanned: ${result.filesScanned}`);
  console.log(`Megabytes scanned: ${megabytes}`);
  console.log(`Hits: ${result.hits.length}`);
  console.log(`Unreadable: ${result.unreadable.length}`);
  for (const item of result.unreadable.slice(0, 50)) {
    console.log(`  ${item.code} ${item.path}`);
  }
  console.log(`Skipped links: ${result.skippedLinks.length}`);
  console.log(`Missing folders: ${result.foldersMissing.length}`);

  const code = exitCodeFor(result);
  if (code === 1) {
    console.log("RESULT: HITS FOUND");
  } else if (code === 3) {
    console.log(
      `RESULT: NO HITS BUT INCOMPLETE - ${result.unreadable.length} items could not be read`,
    );
  } else {
    console.log("RESULT: NO HITS - complete scan");
  }
  return code;
}

function run(argv) {
  const command = argv[0];
  if (command === "gen") {
    return runGen(argv.slice(1));
  }
  if (command === "scan") {
    return runScan(argv.slice(1));
  }
  return usage("Unknown command.");
}

const invokedDirectly =
  process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  process.exitCode = run(process.argv.slice(2));
}
