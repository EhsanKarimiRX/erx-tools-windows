// Tests for the canary scanner. Written before the tool (TDD).
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  buildPatterns,
  buildAllPatterns,
  nameNeedles,
  scanFile,
  scanFolders,
  exitCodeFor,
  parseCanaryFile,
  generateCanaries,
} from "./canary-scan.mjs";

const TEST_CANARY = "ERXCANARY-TESTONLY-0123456789";
const tempDirs = [];

function tempDir(label) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `erx-canary-test-${label}-`));
  tempDirs.push(dir);
  return dir;
}

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

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

function fakeDirent(name, kind) {
  return {
    name,
    isSymbolicLink: () => kind === "link",
    isDirectory: () => kind === "dir",
    isFile: () => kind === "file",
  };
}

function makeFakeFs({ dirs, files, failOpen = new Set(), failReaddir = new Set() }) {
  const handles = new Map();
  const opened = [];
  let nextFd = 1;
  return {
    opened,
    readdirSync(dir) {
      if (failReaddir.has(dir)) {
        const err = new Error("denied");
        err.code = "EACCES";
        throw err;
      }
      if (!(dir in dirs)) {
        const err = new Error("missing");
        err.code = "ENOENT";
        throw err;
      }
      return dirs[dir];
    },
    openSync(filePath) {
      opened.push(filePath);
      if (failOpen.has(filePath)) {
        const err = new Error("busy");
        err.code = "EBUSY";
        throw err;
      }
      const fd = nextFd;
      nextFd += 1;
      handles.set(fd, { path: filePath, pos: 0 });
      return fd;
    },
    readSync(fd, buffer, offset, length) {
      const handle = handles.get(fd);
      const data = files[handle.path] ?? Buffer.alloc(0);
      const count = Math.max(0, Math.min(length, data.length - handle.pos));
      if (count > 0) {
        data.copy(buffer, offset, handle.pos, handle.pos + count);
        handle.pos += count;
      }
      return count;
    },
    closeSync(fd) {
      handles.delete(fd);
    },
  };
}

function sortHits(hits) {
  return hits
    .map((hit) => ({
      kind: hit.kind,
      pattern: hit.pattern,
      canary: hit.canary,
      path: hit.path,
    }))
    .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

test("T1 buildPatterns for an ASCII canary has utf8 and utf16le only", () => {
  const patterns = buildPatterns(TEST_CANARY);
  assert.deepEqual(patterns.map((p) => p.name), ["utf8", "utf16le"]);
});

test("T2 buildPatterns for the Persian canary has all four encodings", () => {
  const canary = generateCanaries("deadbeef")[1];
  const patterns = buildPatterns(canary);
  assert.deepEqual(
    patterns.map((p) => p.name),
    ["utf8", "utf16le", "json-escape", "url-encoded"],
  );
  const byName = Object.fromEntries(patterns.map((p) => [p.name, p.bytes]));
  assert.deepEqual(byName.utf8, Buffer.from(canary, "utf8"));
  assert.deepEqual(byName.utf16le, Buffer.from(canary, "utf16le"));
  const jsonText = byName["json-escape"].toString("utf8");
  assert.ok(jsonText.includes("\\u0633"));
  assert.ok(jsonText.includes("\\u200c"));
  const urlText = byName["url-encoded"].toString("utf8");
  assert.ok(urlText.includes("%D8%B3"));
  assert.ok(urlText.includes("%E2%80%8C"));
});

test("T3 buildPatterns enforces the minimum canary length", () => {
  assert.throws(() => buildPatterns("x".repeat(11)), Error);
  assert.doesNotThrow(() => buildPatterns("x".repeat(12)));
});

test("T4 generateCanaries builds three distinct strings", () => {
  const canaries = generateCanaries("aaaaaaaa");
  assert.equal(canaries.length, 3);
  for (const canary of canaries) {
    assert.ok(canary.includes("aaaaaaaa"));
    assert.ok(canary.length >= 12);
  }
  assert.ok(canaries[1].includes("\u200c"));
  const other = generateCanaries("bbbbbbbb");
  assert.notDeepEqual(canaries, other);
});

test("T5 parseCanaryFile strips BOM, comments, and empty lines", () => {
  const ascii = "ERXCANARY-BOMTEST-ENGLISH-hello-world";
  const persian = generateCanaries("bomtest")[1];
  const text = "\uFEFF" + ascii + "\r\n\r\n# a comment\r\n" + persian + "\r\n";
  assert.deepEqual(parseCanaryFile(text), [ascii, persian]);
});

test("T6 scanFile reports the utf8 offset", () => {
  const dir = tempDir("t6");
  const file = path.join(dir, "a.bin");
  fs.writeFileSync(file, Buffer.concat([Buffer.from("xxxx", "utf8"), Buffer.from(TEST_CANARY, "utf8")]));
  const result = scanFile(file, buildPatterns(TEST_CANARY));
  assert.equal(result.hits.length, 1);
  assert.equal(result.hits[0].pattern, "utf8");
  assert.equal(result.hits[0].offset, 4);
  assert.equal(result.bytes, 4 + TEST_CANARY.length);
});

test("T7 scanFile finds the utf16le pattern at its offset", () => {
  const dir = tempDir("t7");
  const file = path.join(dir, "b.bin");
  const data = Buffer.concat([
    Buffer.alloc(5000, 0x41),
    Buffer.from(TEST_CANARY, "utf16le"),
    Buffer.alloc(100, 0x42),
  ]);
  fs.writeFileSync(file, data);
  const result = scanFile(file, buildPatterns(TEST_CANARY));
  assert.equal(result.hits.length, 1);
  assert.equal(result.hits[0].pattern, "utf16le");
  assert.equal(result.hits[0].offset, 5000);
});

test("T8 scanFile finds matches split across chunk boundaries", () => {
  const dir = tempDir("t8");
  const patterns = buildPatterns(TEST_CANARY);
  for (let offset = 0; offset <= 40; offset += 1) {
    const file = path.join(dir, `split-${offset}.bin`);
    const data = Buffer.concat([
      Buffer.alloc(offset, 0x5A),
      Buffer.from(TEST_CANARY, "utf8"),
      Buffer.alloc(5, 0x59),
    ]);
    fs.writeFileSync(file, data);
    const result = scanFile(file, patterns, { chunkSize: 16 });
    assert.equal(result.hits.length, 1, `offset ${offset}`);
    assert.equal(result.hits[0].pattern, "utf8", `offset ${offset}`);
    assert.equal(result.hits[0].offset, offset, `offset ${offset}`);
  }
});

test("T9 scanFile does not report truncated or lower-case canaries", () => {
  const dir = tempDir("t9");
  const patterns = buildPatterns(TEST_CANARY);
  const truncated = path.join(dir, "truncated.bin");
  const lower = path.join(dir, "lower.bin");
  fs.writeFileSync(truncated, Buffer.from(TEST_CANARY.slice(0, -1), "utf8"));
  fs.writeFileSync(lower, Buffer.from(TEST_CANARY.toLowerCase(), "utf8"));
  assert.deepEqual(scanFile(truncated, patterns).hits, []);
  assert.deepEqual(scanFile(lower, patterns).hits, []);
});

test("T10 scanFolders finds content and name hits", () => {
  const root = tempDir("t10");
  const canaries = generateCanaries("t10token");
  const patterns = buildAllPatterns(canaries);
  const needles = [...new Set(canaries.flatMap((canary) => nameNeedles(canary)))];

  fs.mkdirSync(path.join(root, "nested", "deeper"), { recursive: true });
  fs.writeFileSync(path.join(root, "nested", "a.bin"), Buffer.from(canaries[0], "utf8"));
  fs.writeFileSync(path.join(root, "nested", "deeper", "b.bin"), Buffer.from(canaries[0], "utf16le"));
  fs.writeFileSync(path.join(root, "c.txt"), Buffer.from(jsonEscape(canaries[1]), "utf8"));
  fs.writeFileSync(path.join(root, "d.txt"), Buffer.from(encodeURIComponent(canaries[1]), "utf8"));
  fs.writeFileSync(path.join(root, "clean.txt"), Buffer.from("nothing to see here", "utf8"));
  fs.writeFileSync(path.join(root, "empty.txt"), Buffer.alloc(0));
  const canaryFolder = path.join(root, canaries[0]);
  fs.mkdirSync(canaryFolder);
  const missing = path.join(root, "does-not-exist");

  const result = scanFolders([root, missing], patterns, needles);
  const actual = sortHits(
    result.hits.map((hit) => ({
      kind: hit.kind,
      pattern: hit.pattern,
      canary: hit.canary,
      path: path.relative(root, hit.path),
    })),
  );
  const expected = [
    { kind: "content", pattern: "json-escape", canary: 2, path: "c.txt" },
    { kind: "content", pattern: "url-encoded", canary: 2, path: "d.txt" },
    { kind: "name", pattern: "name", canary: 1, path: canaries[0] },
    { kind: "content", pattern: "utf16le", canary: 1, path: path.join("nested", "deeper", "b.bin") },
    { kind: "content", pattern: "utf8", canary: 1, path: path.join("nested", "a.bin") },
  ].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));

  assert.deepEqual(actual, expected);
  assert.equal(result.filesScanned, 6);
  assert.deepEqual(result.unreadable, []);
  assert.deepEqual(result.foldersMissing, [missing]);
});

test("T11 injected fs reports an unreadable file and keeps scanning", () => {
  const root = path.join(os.tmpdir(), "erx-canary-fake-t11");
  const okPath = path.join(root, "ok.bin");
  const lockedPath = path.join(root, "locked.bin");
  const dirs = {
    [root]: [fakeDirent("ok.bin", "file"), fakeDirent("locked.bin", "file")],
  };
  const files = { [okPath]: Buffer.from(TEST_CANARY, "utf8") };
  const patterns = buildPatterns(TEST_CANARY);
  const needles = nameNeedles(TEST_CANARY);

  const fakeFs = makeFakeFs({ dirs, files, failOpen: new Set([lockedPath]) });
  const result = scanFolders([root], patterns, needles, { fs: fakeFs });
  assert.equal(result.hits.length, 1);
  assert.equal(result.hits[0].path, okPath);
  assert.equal(result.hits[0].pattern, "utf8");
  assert.equal(result.unreadable.length, 1);
  assert.equal(result.unreadable[0].code, "EBUSY");
  assert.ok(result.unreadable[0].path.endsWith("locked.bin"));
  assert.equal(result.filesScanned, 1);
  assert.equal(exitCodeFor(result), 1);

  const cleanFiles = { [okPath]: Buffer.from("clean content", "utf8") };
  const cleanFs = makeFakeFs({ dirs, files: cleanFiles, failOpen: new Set([lockedPath]) });
  const cleanResult = scanFolders([root], patterns, needles, { fs: cleanFs });
  assert.equal(cleanResult.hits.length, 0);
  assert.equal(cleanResult.unreadable.length, 1);
  assert.equal(exitCodeFor(cleanResult), 3);
});

test("T12 injected fs records an unreadable folder and continues", () => {
  const root = path.join(os.tmpdir(), "erx-canary-fake-t12");
  const sub = path.join(root, "sub");
  const okPath = path.join(root, "ok.bin");
  const dirs = {
    [root]: [fakeDirent("sub", "dir"), fakeDirent("ok.bin", "file")],
    [sub]: [],
  };
  const files = { [okPath]: Buffer.from(TEST_CANARY, "utf8") };
  const fakeFs = makeFakeFs({ dirs, files, failReaddir: new Set([sub]) });

  const result = scanFolders([root], buildPatterns(TEST_CANARY), nameNeedles(TEST_CANARY), {
    fs: fakeFs,
  });
  assert.equal(result.hits.length, 1);
  assert.equal(result.hits[0].path, okPath);
  assert.ok(result.unreadable.some((item) => item.path === sub && item.code === "EACCES"));
});

test("T13 injected fs skips symbolic links without opening them", () => {
  const root = path.join(os.tmpdir(), "erx-canary-fake-t13");
  const linkPath = path.join(root, "link.bin");
  const okPath = path.join(root, "ok.bin");
  const dirs = {
    [root]: [fakeDirent("link.bin", "link"), fakeDirent("ok.bin", "file")],
  };
  const files = { [okPath]: Buffer.from("clean content", "utf8") };
  const fakeFs = makeFakeFs({ dirs, files });

  const result = scanFolders([root], buildPatterns(TEST_CANARY), nameNeedles(TEST_CANARY), {
    fs: fakeFs,
  });
  assert.deepEqual(result.skippedLinks, [linkPath]);
  assert.ok(!fakeFs.opened.includes(linkPath));
});

test("T14 exitCodeFor prioritises hits over unreadable items", () => {
  assert.equal(exitCodeFor({ hits: [{}], unreadable: [{}] }), 1);
  assert.equal(exitCodeFor({ hits: [], unreadable: [{}] }), 3);
  assert.equal(exitCodeFor({ hits: [], unreadable: [] }), 0);
});

test("T15 empty and tiny files do not crash", () => {
  const dir = tempDir("t15");
  const empty = path.join(dir, "empty.bin");
  const three = path.join(dir, "three.bin");
  fs.writeFileSync(empty, Buffer.alloc(0));
  fs.writeFileSync(three, Buffer.from("abc", "utf8"));
  const patterns = buildPatterns(TEST_CANARY);
  const emptyResult = scanFile(empty, patterns);
  const threeResult = scanFile(three, patterns);
  assert.deepEqual(emptyResult.hits, []);
  assert.equal(emptyResult.bytes, 0);
  assert.deepEqual(threeResult.hits, []);
  assert.equal(threeResult.bytes, 3);
});
