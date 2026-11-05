// Shared by the end-to-end suites: results, fixtures, waiting.

const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");

const repo = path.resolve(__dirname, "../..");
const bin = path.join(repo, "target/debug/stagewright" + (process.platform === "win32" ? ".exe" : ""));

let failed = 0;

/** Print one result; a failure is counted for `finish`. */
function check(name, ok, detail = "") {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : " — " + detail}`);
  if (!ok) failed++;
}

/** Count a failure that wasn't a `check` (an exception, say). */
function fail() {
  failed++;
}

function failures() {
  return failed;
}

/** Print the summary and exit with the right code. */
function finish() {
  console.log(failed ? `\n${failed} check(s) failed` : "\nall checks passed");
  process.exit(failed ? 1 : 0);
}

function copyDir(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const e of fs.readdirSync(from, { withFileTypes: true })) {
    const a = path.join(from, e.name),
      b = path.join(to, e.name);
    e.isDirectory() ? copyDir(a, b) : fs.copyFileSync(a, b);
  }
}

/** Wait until `fn()` is truthy (it may be async). */
async function until(fn, ms = 6000, what = "condition") {
  const end = Date.now() + ms;
  for (;;) {
    if (await fn()) return;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

const handedOut = new Set();

/** A port in `base`..`base + span` not already given out by this run, so two
 * servers in one suite can't end up on the same port. */
// Ports Chromium refuses to load (net::ERR_UNSAFE_PORT) in the ranges the suites use.
const UNSAFE = new Set([4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668, 6669, 6679, 6697]);

function pickPort(base, span) {
  for (;;) {
    const p = base + Math.floor(Math.random() * span);
    if (!handedOut.has(p) && !UNSAFE.has(p)) {
      handedOut.add(p);
      return p;
    }
  }
}

/** Wait until something is listening on `port` (a server just spawned). */
async function serverUp(port, ms = 10000) {
  await until(
    () =>
      new Promise((resolve) => {
        const s = net.connect(port, "127.0.0.1");
        s.once("connect", () => (s.destroy(), resolve(true)));
        s.once("error", () => resolve(false));
      }),
    ms,
    `a server on port ${port}`,
  );
}

module.exports = { repo, bin, check, fail, failures, finish, copyDir, until, serverUp, pickPort };
