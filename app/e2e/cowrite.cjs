// End-to-end co-writing: two writers, two folders, two app instances, one relay.
//
//   cargo build -p stagewright && npm run build && node e2e/cowrite.cjs
//
// Needs Playwright with Chromium available (require('playwright')).

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, until, serverUp, pickPort } = require("./lib.cjs");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const dist = path.join(repo, "app/dist");
const shots = process.env.SHOTS_DIR;
const base = pickPort(5100, 400);
const ports = { relay: base, a: base + 1, b: base + 2 };




/** Folder contents that must match between writers: act folders, file names, bodies, key metadata. */
function folderState(root) {
  const out = {};
  const acts = path.join(root, "acts");
  for (const a of fs.readdirSync(acts).sort()) {
    for (const f of fs.readdirSync(path.join(acts, a)).sort()) {
      if (!f.endsWith(".scene")) continue;
      const text = fs.readFileSync(path.join(acts, a, f), "utf8");
      const end = text.indexOf("\n---\n", 4);
      const fm = text.slice(4, end);
      // Compare values, not YAML spelling ("draft" and draft are the same).
      const pick = (k) => {
        const raw = (fm.match(new RegExp(`^${k}: (.*?)(\\s+#.*)?$`, "m")) || [])[1];
        if (raw === undefined) return null;
        try {
          return JSON.parse(raw);
        } catch {
          return raw;
        }
      };
      out[`${a}/${f}`] = {
        id: pick("id"),
        title: pick("title"),
        status: pick("status"),
        duration: pick("duration"),
        body: text.slice(end + 5),
      };
    }
  }
  for (const f of ["project.yaml", "characters.yaml", "locations.yaml", "songs.yaml", "comments.yaml"]) {
    const p = path.join(root, f);
    if (fs.existsSync(p)) out[f] = fs.readFileSync(p, "utf8");
  }
  return out;
}

/** The whole text in a page's editor (CodeMirror only renders what is visible). */
const editorText = (page) => page.evaluate(() => (window.__swText ? window.__swText() : ""));

function diff(a, b) {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  const d = [];
  for (const k of keys) if (JSON.stringify(a[k]) !== JSON.stringify(b[k])) d.push(k);
  return d;
}

function sceneFile(root, id) {
  const acts = path.join(root, "acts");
  for (const a of fs.readdirSync(acts)) {
    for (const f of fs.readdirSync(path.join(acts, a))) {
      const p = path.join(acts, a, f);
      if (f.endsWith(".scene") && fs.readFileSync(p, "utf8").includes(`\nid: ${id}\n`)) return p;
    }
  }
  return null;
}

(async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sw-cowrite-"));
  const dirA = path.join(tmp, "liam");
  const dirB = path.join(tmp, "sam", "harbour-lights");
  const relayData = path.join(tmp, "relay-data");
  copyDir(path.join(repo, "fixtures/stage-sample"), dirA);
  fs.mkdirSync(path.dirname(dirB), { recursive: true });

  const procs = [];
  const startRelay = () => {
    const p = spawn(bin, ["relay", "--port", String(ports.relay), "--data", relayData], { stdio: "pipe" });
    procs.push(p);
    return p;
  };
  let relay = startRelay();
  procs.push(spawn(bin, ["serve", dirA, "--port", String(ports.a), "--static", dist], { stdio: "pipe" }));
  procs.push(spawn(bin, ["serve", "--port", String(ports.b), "--static", dist], { stdio: "pipe" }));
  await Promise.all([serverUp(ports.a), serverUp(ports.b), serverUp(ports.relay)]);

  const browser = await chromium.launch();
  const errors = [];
  const open = async (port, who) => {
    const page = await browser.newPage({ viewport: { width: 1400, height: 880 } });
    page.on("pageerror", (e) => errors.push(`${who}: ${e}`));
    page.on("console", (m) => {
      const t = m.text();
      // Expected noise: the relay being stopped on purpose, and 400s from refused API calls.
      if (m.type() === "error" && !/status of 400|WebSocket|ERR_CONNECTION_REFUSED/.test(t)) errors.push(`${who}: ${t}`);
    });
    await page.goto(`http://127.0.0.1:${port}/`);
    return page;
  };
  const snap = async (page, n) => shots && (await page.screenshot({ path: path.join(shots, n) }));

  try {
    // ---- Liam shares
    const A = await open(ports.a, "A");
    await A.waitForSelector(".cm-content");
    await A.click('button:has-text("Share…")');
    await A.fill('.dialog input[type="text"] >> nth=0', "Liam");
    await A.fill('.dialog input[type="text"] >> nth=1', `ws://127.0.0.1:${ports.relay}`);
    await A.click('.dialog button:has-text("Start sharing")');
    const code = (await (await A.waitForSelector('[data-testid="invite-code"]')).inputValue()).trim();
    check("sharing produces an invite code", code.startsWith("sw1."), code);
    await A.click('.dialog button:has-text("Done")');
    await until(async () => (await A.textContent('[data-testid="collab-status"]')).includes("Live"), 8000, "A live");
    check(".stagewright is git-ignored", fs.readFileSync(path.join(dirA, ".stagewright/.gitignore"), "utf8") === "*\n");

    // ---- Sam joins into an empty folder
    const B = await open(ports.b, "B");
    await B.click('button:has-text("Join a shared project…")');
    await B.fill('[data-testid="join-code"]', code);
    await B.fill('.card input[type="text"] >> nth=0', "Sam");
    await B.fill('[data-testid="join-root"]', dirB);
    await B.click('button:has-text("Join")');
    await B.waitForSelector(".cm-content", { timeout: 15000 });
    await until(() => {
      const d = diff(folderState(dirA), folderState(dirB));
      return d.length === 0 || (void (globalThis.lastDiff = d));
    }, 15000, "B's folder to match A's").catch((e) => {
      const k = globalThis.lastDiff[0];
      const a = folderState(dirA)[k], b = folderState(dirB)[k];
      const show = (x) => JSON.stringify(x && typeof x === "object" ? { ...x, body: x.body.slice(0, 120) } : x);
      check("joined folder matches the sharer's", false, `${e.message}; differs: ${globalThis.lastDiff}\nA ${show(a)}\nB ${show(b)}`);
      if (process.env.STOP_AT_JOIN) throw e;
    });
    if (!diff(folderState(dirA), folderState(dirB)).length) check("joined folder matches the sharer's", true);
    await until(async () => (await A.$$(".presence .avatar")).length === 1, 5000, "A sees Sam");
    check("each writer sees the other", (await A.getAttribute(".presence .avatar", "title")).startsWith("Sam"));

    // ---- Live typing both ways in the same scene
    await A.click('[data-scene-id="sc-1-3"] .open');
    await B.click('[data-scene-id="sc-1-3"] .open');
    await A.waitForTimeout(400);
    await A.click(".cm-content");
    await A.keyboard.press("Control+End");
    await A.keyboard.type(" Liam was here.");
    await until(async () => (await editorText(B)).includes("Liam was here."), 5000, "B to see A's typing");
    check("A's typing appears live in B's editor", true);
    await B.click(".cm-content");
    await B.keyboard.press("Control+Home");
    await B.keyboard.type("Sam was here. ");
    await until(async () => (await editorText(A)).includes("Sam was here."), 5000, "A to see B's typing");
    check("B's typing appears live in A's editor", true);
    await until(() => {
      const fa = fs.readFileSync(sceneFile(dirA, "sc-1-3"), "utf8");
      const fb = fs.readFileSync(sceneFile(dirB, "sc-1-3"), "utf8");
      return fa.includes("Sam was here. ") && fa.includes("Liam was here.") && fb.includes("Sam was here. ") && fb.includes("Liam was here.");
    }, 8000, "both files to contain both edits");
    check("both writers' files contain both edits", true);
    await until(async () => (await A.$$(".scene .peer")).length >= 1, 4000, "A sees where Sam is");
    check("the scene tree shows where the other writer is", true);

    // ---- Comments are shared: Liam comments, Sam sees it and replies
    await A.evaluate(() => window.__swSelect("Liam was here."));
    await A.click('[data-testid="comment-add"]');
    await A.fill('[data-testid="comment-draft"]', "Too modest?");
    await A.click('[data-testid="comment-submit"]');
    await B.click('[data-testid="comments-toggle"]');
    const bt = B.locator(".card[data-thread]").first();
    await until(async () => (await bt.count()) > 0 && (await bt.textContent()).includes("Too modest?"), 6000, "B to see A's comment");
    check("a comment appears for the other writer, with its author", (await bt.textContent()).includes("Liam"));
    await bt.locator("button.quote").click();
    await until(async () => (await B.locator(".sw-comment").count()) > 0, 4000, "B's highlight");
    check("on the same text", (await B.locator(".sw-comment").first().textContent()) === "Liam was here.");
    await bt.locator('textarea[aria-label="Reply"]').fill("Just right.");
    await bt.locator('button:has-text("Reply")').click();
    await until(async () => (await A.locator(".card[data-thread] .msg").count()) === 2, 6000, "A to see Sam's reply");
    check("a reply reaches the first writer", (await A.locator(".card[data-thread]").first().textContent()).includes("Sam"));
    await until(() => {
      const ya = path.join(dirA, "comments.yaml"), yb = path.join(dirB, "comments.yaml");
      return fs.existsSync(ya) && fs.existsSync(yb) && fs.readFileSync(ya, "utf8").includes("Just right.") && fs.readFileSync(ya, "utf8") === fs.readFileSync(yb, "utf8");
    }, 8000, "both folders' comments.yaml to match");
    check("both folders have the same comments.yaml", true);
    await snap(A, "cowrite-01-A.png");
    await snap(B, "cowrite-01-B.png");

    // ---- Details and structure
    await B.fill('[data-testid="meta-title"]', "On the road");
    await B.press('[data-testid="meta-title"]', "Enter");
    await until(() => fs.existsSync(path.join(dirA, "acts/01/030-on-the-road.scene")), 8000, "A's file to be renamed");
    check("a title change by B renames A's file", true);

    await A.hover('[data-scene-id="sc-2-4"]');
    await A.click('[aria-label="Move Outside the lighthouse up"]');
    await until(() => fs.existsSync(path.join(dirB, "acts/02/030-outside-the-lighthouse.scene")), 8000, "B's order to change");
    check("a move by A reorders B's folder", true);

    await B.click('section[aria-label="Act Two"] .act-head .add');
    await B.waitForTimeout(500);
    await B.click(".cm-content");
    await B.keyboard.type("A brand new scene from Sam.");
    await until(() => {
      const acts2 = fs.readdirSync(path.join(dirA, "acts/02"));
      const f = acts2.find((x) => x.includes("new-scene"));
      return f && fs.readFileSync(path.join(dirA, "acts/02", f), "utf8").includes("A brand new scene from Sam.");
    }, 8000, "B's new scene to reach A");
    check("a scene created by B appears in A's folder with its text", true);

    // ---- Both offline, both edit, reconnect
    relay.kill();
    await until(async () => (await A.textContent('[data-testid="collab-status"]')).includes("Offline") || (await A.textContent('[data-testid="collab-status"]')).includes("Connecting"), 8000, "A to notice the relay is gone");
    await A.click('[data-scene-id="sc-2-2"] .open');
    await B.click('[data-scene-id="sc-2-2"] .open');
    await A.waitForTimeout(300);
    await A.click(".cm-content");
    await A.keyboard.press("Control+Home");
    await A.keyboard.type("[A offline] ");
    await B.click(".cm-content");
    await B.keyboard.press("Control+End");
    await B.keyboard.type(" [B offline]");
    await A.waitForTimeout(1500);
    check(
      "offline edits are saved to each writer's own file",
      fs.readFileSync(sceneFile(dirA, "sc-2-2"), "utf8").includes("[A offline]") &&
        fs.readFileSync(sceneFile(dirB, "sc-2-2"), "utf8").includes("[B offline]"),
    );
    await snap(A, "cowrite-02-offline.png");
    relay = startRelay();
    const body = (f) => {
      const t = fs.readFileSync(f, "utf8");
      return t.slice(t.indexOf("\n---\n", 4) + 5);
    };
    await until(() => {
      const fa = body(sceneFile(dirA, "sc-2-2"));
      const fb = body(sceneFile(dirB, "sc-2-2"));
      return fa === fb && fa.includes("[A offline]") && fa.includes("[B offline]");
    }, 20000, "offline edits to converge");
    check("offline edits on both sides merge after reconnecting", true);

    // ---- An edit made outside the app
    const ext = sceneFile(dirA, "sc-1-2");
    fs.writeFileSync(ext, fs.readFileSync(ext, "utf8") + "\nEdited in another program.\n");
    await until(() => fs.readFileSync(sceneFile(dirB, "sc-1-2"), "utf8").includes("Edited in another program."), 12000, "the outside edit to reach B");
    check("an edit made outside the app reaches the other writer", true);

    // ---- A renames a character in the Cast view; B's folder follows
    await A.click(`nav[aria-label="Views"] >> text=Cast`);
    await A.click('[aria-label="Rename CORNELIUS"]');
    await A.fill('[data-testid="rename-to"]', "CORNELIA");
    await A.click('[data-testid="rename-preview-btn"]');
    await A.click('[data-testid="rename-apply"]');
    await until(
      () =>
        fs.readFileSync(path.join(dirB, "characters.yaml"), "utf8").includes("cue: CORNELIA") &&
        /\nCORNELIA\n/.test(fs.readFileSync(sceneFile(dirB, "sc-2-1"), "utf8")),
      12000,
      "the rename to reach B",
    );
    check("a rename while co-writing reaches the other writer", !/\bCORNELIUS\b/.test(fs.readFileSync(sceneFile(dirB, "sc-2-1"), "utf8")));
    await A.click(`nav[aria-label="Views"] >> text=Script`);

    // ---- Settled: both folders identical
    await until(() => diff(folderState(dirA), folderState(dirB)).length === 0, 10000, "folders to settle");
    check("both folders end identical", true);

    // ---- Restart A's app: the session resumes from .stagewright
    await A.reload();
    await A.waitForSelector(".cm-content");
    await until(async () => (await A.textContent('[data-testid="collab-status"]')).includes("Live"), 8000, "A to resume");
    check("co-writing resumes when the project is reopened", true);

    check("no unexpected browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    try {
      const pages = browser.contexts().flatMap((c) => c.pages());
      for (const [i, p] of pages.entries()) await snap(p, `cowrite-error-${i}.png`);
    } catch {}
  } finally {
    await browser.close();
    for (const p of procs) p.kill();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  finish();
})();
