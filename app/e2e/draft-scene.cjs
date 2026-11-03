// End-to-end check: draft a whole scene in the browser using only keys and
// toolbar buttons, then verify the file on disk with the Rust parser.
//
//   cargo build -p stagewright && npm run build && node e2e/draft-scene.cjs
//
// Needs Playwright with Chromium available (require('playwright')).

const { chromium } = require("playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const repo = path.resolve(__dirname, "../..");
const bin = path.join(repo, "target/debug/stagewright" + (process.platform === "win32" ? ".exe" : ""));
const port = 4890 + Math.floor(Math.random() * 50);
const shots = process.env.SHOTS_DIR;

function copyDir(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const e of fs.readdirSync(from, { withFileTypes: true })) {
    const a = path.join(from, e.name), b = path.join(to, e.name);
    e.isDirectory() ? copyDir(a, b) : fs.copyFileSync(a, b);
  }
}

let failures = 0;
function check(name, ok, detail = "") {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : " — " + detail}`);
  if (!ok) failures++;
}

(async () => {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-e2e-"));
  copyDir(path.join(repo, "fixtures/stage-sample"), project);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await new Promise((r) => setTimeout(r, 800));

  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  // 400s are expected: the conflict and invalid-YAML steps provoke them on purpose.
  page.on("console", (m) => m.type() === "error" && !/status of 400/.test(m.text()) && errors.push(m.text()));
  const snap = async (n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  const saved = () => page.waitForFunction(() => document.querySelector('[data-testid="save-status"]')?.textContent === "Saved", null, { timeout: 5000 });
  const act2 = () => path.join(project, "acts/02");
  const findFile = (re) => fs.readdirSync(act2()).find((f) => re.test(f));

  try {
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForSelector(".cm-content");

    // 1. New scene at the end of Act Two
    await page.click('section[aria-label="Act Two"] .act-head .add');
    await page.waitForFunction(() => document.querySelector('[data-testid="meta-title"]')?.value === "New scene");
    check("new scene created and opened", !!findFile(/^050-new-scene\.scene$/), fs.readdirSync(act2()).join(","));

    // 2. Rename it from the details panel
    await page.fill('[data-testid="meta-title"]', "The confession");
    await page.press('[data-testid="meta-title"]', "Enter");
    await page.waitForFunction(() => [...document.querySelectorAll(".scene .name")].some((e) => e.textContent === "The confession"));
    check("title change renames the file", !!findFile(/^050-the-confession\.scene$/), fs.readdirSync(act2()).join(","));

    // 3. Draft with keys only
    await page.click(".cm-content");
    const kb = page.keyboard;
    await kb.type("Night. The lighthouse is empty.");
    await kb.press("Enter"); // new paragraph
    await kb.press("Tab"); // start a speech
    await kb.type("morde"); // upper-cased, completion opens
    await page.waitForSelector(".cm-tooltip-autocomplete");
    await kb.press("Enter"); // accept MORDECAI, move to dialogue
    await kb.type("Is anyone there?");
    await kb.press("Enter"); // end speech
    await kb.press("Tab");
    await kb.type("rosalind");
    await page.waitForSelector(".cm-tooltip-autocomplete");
    await kb.press("Tab"); // Tab also accepts
    await kb.press("Tab"); // empty dialogue line -> ()
    await kb.type("from the shadows");
    await kb.press("End");
    await kb.press("Enter"); // to dialogue
    await kb.type("Only me. And the *rain*.");
    await kb.press("Enter");
    // a song via the toolbar
    await kb.press("Tab");
    await kb.type("hettie");
    await page.waitForSelector(".cm-tooltip-autocomplete");
    await kb.press("Enter");
    await kb.type("singing");
    await kb.press("Tab"); // dialogue -> parenthetical
    await page.click('[data-element="song"]');
    await kb.type("Small Mercies");
    await kb.press("ArrowDown");
    await kb.press("End");
    await kb.type("I came to save an island");
    await kb.press("Enter");
    await kb.type("I'd only seen on maps");
    await kb.press("Enter");
    await kb.press("Enter"); // empty lyric ends the song
    await kb.type("blackout");
    await page.click('[data-element="transition"]');
    await snap("e2e-01-drafted.png");
    await saved();

    const file = path.join(act2(), findFile(/the-confession/));
    const text = fs.readFileSync(file, "utf8");
    const body = text.slice(text.indexOf("\n---\n", 4) + 5);
    const expected = [
      "Night. The lighthouse is empty.",
      "",
      "MORDECAI",
      "Is anyone there?",
      "",
      "ROSALIND",
      "(from the shadows)",
      "Only me. And the *rain*.",
      "",
      "HETTIE",
      "(singing)",
      '~ SONG "Small Mercies" [HETTIE]',
      "~ I came to save an island",
      "~ I'd only seen on maps",
      "~ END SONG",
      "",
      "> BLACKOUT",
    ].join("\n");
    check("drafted body is exactly right on disk", body === expected, "\n--- got ---\n" + body + "\n--- expected ---\n" + expected);

    const kinds = JSON.parse(execFileSync(bin, ["classify", file]).toString())[path.basename(file)];
    check(
      "Rust parser reads the drafted scene as intended",
      JSON.stringify(kinds) ===
        JSON.stringify([
          "direction", "blank", "cue", "dialogue", "blank", "cue", "parenthetical", "dialogue", "blank",
          "cue", "parenthetical", "song_start", "lyric", "lyric", "song_end", "blank", "transition",
        ]),
      JSON.stringify(kinds),
    );

    // 3b. Switching to another view and back keeps the editor exactly as it was,
    // including typing that has not been saved yet.
    const views = (name) => page.click(`nav[aria-label="Views"] >> text=${name}`);
    await views("Production");
    await page.waitForSelector("table.times");
    await views("Script");
    check("text is still there after Production → Script", (await page.evaluate(() => window.__swText())) === expected);
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.type(" Unsaved.");
    await views("Production"); // straight away, before the autosave
    await page.waitForSelector("table.times");
    await views("Script");
    check("unsaved typing survives a view switch", (await page.evaluate(() => window.__swText())).endsWith("> BLACKOUT Unsaved."));
    await page.click(".cm-content");
    await kb.press("Control+End");
    for (let i = 0; i < " Unsaved.".length; i++) await kb.press("Backspace");
    await saved();
    check("and the file matches the editor", fs.readFileSync(file, "utf8").endsWith(expected), JSON.stringify(fs.readFileSync(file, "utf8").slice(-80)) + " editor: " + JSON.stringify((await page.evaluate(() => window.__swText())).slice(-60)));
    const status = await page.textContent('[data-testid="element"]');
    check("status bar shows the element at the cursor", status === "Transition", status);

    // 4. Metadata: manual duration
    await page.fill('[data-testid="meta-duration"]', "6");
    await page.press('[data-testid="meta-duration"]', "Enter");
    await page.waitForFunction(() => document.querySelector('[data-scene-id] .mins')?.textContent !== null);
    await page.waitForTimeout(300);
    check("duration written to frontmatter", /\nduration: 6  # manual override, minutes\n/.test(fs.readFileSync(path.join(act2(), findFile(/the-confession/)), "utf8")));

    // 5. Move it to the top of Act Two with the keyboard-accessible buttons
    const position = () =>
      page.evaluate(() => {
        const li = [...document.querySelectorAll("[data-scene-id]")].find((e) => e.textContent.includes("The confession"));
        return li ? [...li.parentElement.children].indexOf(li) : -1;
      });
    for (let i = 0; i < 4; i++) {
      const before = await position();
      await page.hover(`[data-scene-id] >> text=The confession`);
      await page.click(`[aria-label="Move The confession up"]`);
      // Wait for the move to land before the next one (slow machines take a while).
      for (let t = 0; t < 50 && (await position()) === before; t++) await page.waitForTimeout(100);
    }
    // The moves are saved one after another; give a slow machine time to finish them.
    for (let t = 0; t < 50 && !findFile(/^010-the-confession\.scene$/); t++) await page.waitForTimeout(100);
    check("moved to the top of the act", !!findFile(/^010-the-confession\.scene$/), fs.readdirSync(act2()).join(","));
    check("other scenes renumbered", !!findFile(/^050-outside-the-lighthouse\.scene$/), fs.readdirSync(act2()).join(","));

    // 6. Editing still saves after the move (version refreshed)
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Enter");
    await kb.type("The lights fade.");
    await saved();
    check("edits after a move still save", fs.readFileSync(path.join(act2(), findFile(/the-confession/)), "utf8").endsWith("> BLACKOUT\n\nThe lights fade."));

    // 7. Someone else edits the file: conflict, then keep mine
    const f2 = path.join(act2(), findFile(/the-confession/));
    fs.writeFileSync(f2, fs.readFileSync(f2, "utf8") + "\n\nWritten in another editor.");
    await kb.type(" Silence.");
    await page.waitForSelector(".conflict", { timeout: 5000 });
    await snap("e2e-02-conflict.png");
    check("external change is detected as a conflict", true);
    await page.click('.conflict button:has-text("Keep mine")');
    await saved();
    const after = fs.readFileSync(f2, "utf8");
    check("keep mine overwrites with the editor's text", after.endsWith("The lights fade. Silence.") && !after.includes("another editor"), after.slice(-80));

    // 8. Reload the page: content persists
    await page.reload();
    await page.waitForSelector(".cm-content");
    await page.click(`[data-scene-id] >> text=The confession`);
    await page.waitForFunction(() => document.querySelector(".cm-content")?.textContent?.includes("Small Mercies"));
    check("content survives a reload", true);

    // 8b. Typing a new paragraph above an existing one is not mistaken for a cue
    await page.click(".cm-content");
    await kb.press("Control+Home");
    await kb.press("Enter");
    await kb.press("ArrowUp");
    await kb.type("Rain falls. ");
    await kb.press("Enter");
    await saved();
    const top = fs.readFileSync(path.join(act2(), findFile(/the-confession/)), "utf8").split("\n---\n")[1];
    check("a sentence typed above a paragraph stays in normal case", top.startsWith("Rain falls. \n\n\nNight."), JSON.stringify(top.slice(0, 40)));

    // 9. Production view includes the new scene
    await page.click('nav button:has-text("Production")');
    await page.waitForSelector(".matrix");
    const prod = await page.textContent(".times");
    check("production view lists the new scene", prod.includes("The confession") && prod.includes("Small Mercies"));
    await snap("e2e-03-production.png");

    // 10. Bad YAML is refused with a message
    await page.selectOption('select[aria-label="Project files"]', "characters.yaml");
    await page.waitForSelector(".file .cm-content");
    await page.click(".file .cm-content");
    await kb.press("Control+End");
    await kb.type("\n- {cue: NOID}");
    await page.click('.file button:has-text("Save")');
    await page.waitForSelector(".problem");
    check("invalid characters.yaml is refused", (await page.textContent(".problem")).includes("Not saved"));

    check("no browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    failures++;
    await snap("e2e-error.png");
  } finally {
    await browser.close();
    server.kill();
    fs.rmSync(project, { recursive: true, force: true });
  }
  console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
  process.exit(failures ? 1 : 0);
})();
