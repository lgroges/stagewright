#!/usr/bin/env node
// Version numbers for Stagewright live in one place (Cargo.toml's
// [workspace.package]) and are copied to app/package.json, the lockfiles and
// CHANGELOG.md by this script.
//
//   node scripts/release.mjs 0.3.0     bump, date the changelog, commit, tag v0.3.0
//   node scripts/release.mjs --check   fail if the copies disagree (CI runs this)
//   node scripts/release.mjs --notes 0.3.0   print that version's changelog section
//
// Then push main and start the release workflow (Actions → release → Run
// workflow), or push the tag (git push origin main v0.3.0). It builds the
// installers and publishes them where the app looks for updates.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const file = (p) => join(root, p);
const read = (p) => readFileSync(file(p), "utf8");
const write = (p, s) => writeFileSync(file(p), s);
const SEMVER = /^\d+\.\d+\.\d+$/;
const CRATES = ["stagewright", "stagewright-api", "stagewright-app", "stagewright-core"];

function cargoVersion() {
  const m = read("Cargo.toml").match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/);
  if (!m) throw new Error("no version in Cargo.toml [workspace.package]");
  return m[1];
}

function versions() {
  const pkg = JSON.parse(read("app/package.json")).version;
  const lockJson = JSON.parse(read("app/package-lock.json"));
  const lock = read("Cargo.lock");
  const crates = CRATES.map((c) => lock.match(new RegExp(`name = "${c}"\\nversion = "([^"]+)"`))?.[1] ?? "missing");
  return { cargo: cargoVersion(), pkg, pkgLock: lockJson.version, pkgLockRoot: lockJson.packages?.[""]?.version, crates };
}

function check() {
  const v = versions();
  const all = [v.pkg, v.pkgLock, v.pkgLockRoot, ...v.crates];
  const bad = all.filter((x) => x !== v.cargo);
  if (bad.length) {
    console.error(`Version mismatch: Cargo.toml says ${v.cargo}, but found`, JSON.stringify(v));
    console.error("Run: node scripts/release.mjs <version>");
    process.exit(1);
  }
  if (!SEMVER.test(v.cargo)) throw new Error(`not a version: ${v.cargo}`);
  console.log(`versions agree: ${v.cargo}`);
}

/** The changelog section for `version` (without its heading). */
function notes(version) {
  const text = read("CHANGELOG.md");
  const start = text.search(new RegExp(`^## ${version.replace(/\./g, "\\.")}\\b.*$`, "m"));
  if (start < 0) return "";
  const body = text.slice(text.indexOf("\n", start) + 1);
  const end = body.search(/^## /m);
  return (end < 0 ? body : body.slice(0, end)).trim();
}

const newer = (a, b) => {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
};

function release(next) {
  if (!SEMVER.test(next)) throw new Error(`"${next}" is not a version like 0.3.0`);
  const current = cargoVersion();
  if (!newer(next, current)) throw new Error(`${next} is not newer than ${current}`);
  const git = (...a) => execFileSync("git", a, { cwd: root, encoding: "utf8" }).trim();
  if (git("status", "--porcelain")) throw new Error("commit or stash your changes first");

  const changelog = read("CHANGELOG.md");
  const unreleased = changelog.match(/^## Unreleased\n([\s\S]*?)(?=^## |$(?![\s\S]))/m);
  if (!unreleased || !unreleased[1].trim()) throw new Error("CHANGELOG.md has nothing under ## Unreleased");
  const date = new Date().toISOString().slice(0, 10);
  write("CHANGELOG.md", changelog.replace(/^## Unreleased\n/m, `## Unreleased\n\n## ${next} (${date})\n`));

  write("Cargo.toml", read("Cargo.toml").replace(/(\[workspace\.package\][^[]*?\nversion = )"[^"]+"/, `$1"${next}"`));
  let lock = read("Cargo.lock");
  for (const c of CRATES) lock = lock.replace(new RegExp(`(name = "${c}"\\nversion = )"[^"]+"`), `$1"${next}"`);
  write("Cargo.lock", lock);
  const pkg = JSON.parse(read("app/package.json"));
  pkg.version = next;
  write("app/package.json", JSON.stringify(pkg, null, 2) + "\n");
  const pl = JSON.parse(read("app/package-lock.json"));
  pl.version = next;
  if (pl.packages?.[""]) pl.packages[""].version = next;
  write("app/package-lock.json", JSON.stringify(pl, null, 2) + "\n");

  check();
  git("add", "-A");
  git("commit", "-q", "-m", `Release ${next}`);
  git("tag", "-a", `v${next}`, "-m", `Stagewright ${next}\n\n${notes(next)}`);
  console.log(`Tagged v${next}. Publish it: git push origin main v${next}\n(or push main and run the release workflow from GitHub's Actions tab)`);
}

const [arg, ver] = process.argv.slice(2);
try {
  if (arg === "--check") check();
  else if (arg === "--notes") console.log(notes(ver ?? cargoVersion()));
  else if (arg) release(arg);
  else {
    console.log(read("scripts/release.mjs").split("\n").slice(1, 12).join("\n").replace(/^\/\/ ?/gm, ""));
    process.exit(1);
  }
} catch (e) {
  console.error(e.message);
  process.exit(1);
}
