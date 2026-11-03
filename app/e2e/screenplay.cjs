// End-to-end check for film and TV: create a TV series from the welcome
// screen, draft a screenplay scene with keys only, add an episode, read the
// scene breakdown and export the script as PDF and Fountain.
//
//   cargo build -p stagewright && npm run build && node e2e/screenplay.cjs
//
// Needs Playwright with Chromium available (require('playwright')).

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, serverUp } = require("./lib.cjs");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const port = 4950 + Math.floor(Math.random() * 40);
const shots = process.env.SHOTS_DIR;


(async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sw-screen-"));
  const root = path.join(tmp, "night-shift");
  const server = spawn(bin, ["serve", "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);

  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, acceptDownloads: true });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  const snap = async (n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  const saved = () => page.waitForFunction(() => document.querySelector('[data-testid="save-status"]')?.textContent === "Saved", null, { timeout: 5000 });
  const text = () => page.evaluate(() => window.__swText?.() ?? "");
  const kb = page.keyboard;

  try {
    await page.goto(`http://127.0.0.1:${port}/`);

    // 1. New single-camera series from the welcome screen
    await page.click("text=New project…");
    await page.fill('[data-testid="new-title"]', "Night Shift");
    await page.selectOption('[data-testid="new-format"]', "tv-single");
    await page.fill('[data-testid="new-root"]', root);
    await page.click("button.primary:has-text('Create')");
    await page.waitForSelector(".cm-content");
    const yaml = fs.readFileSync(path.join(root, "project.yaml"), "utf8");
    check("project.yaml records the format and series", /format: tv-single/.test(yaml) && /series: true/.test(yaml), yaml);
    const ep1 = path.join(root, "episodes/01/acts/01");
    check("first scene is inside episode 1", fs.existsSync(ep1) && fs.readdirSync(ep1).length === 1, String(fs.existsSync(ep1) && fs.readdirSync(ep1)));
    check("tree shows the episode", (await page.locator(".episode-head").count()) === 1);
    check("toolbar starts with Heading and says Action", /^Heading/.test((await page.locator(".toolbar button").first().textContent()).trim()) && (await page.locator(".toolbar button:has-text('Action')").count()) === 1);

    // 2. Draft a scene with keys only
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Control+8"); // heading
    await kb.type("kitchen");
    await kb.type(" - ");
    await page.waitForSelector(".cm-tooltip-autocomplete"); // time-of-day completion
    await kb.type("nig");
    await kb.press("Enter"); // accept NIGHT (the open list filters as you type)
    await kb.press("Enter"); // heading -> action
    await kb.type("Rain on the window. MARA waits at the table.");
    await kb.press("Enter");
    await kb.press("Tab"); // speech
    await kb.type("mara");
    await kb.press("Enter"); // -> dialogue
    await kb.type("You're late.");
    await kb.press("Enter"); // end speech
    await kb.type("int. kit"); // typed heading, lower case; places come from the document
    await page.waitForSelector(".cm-tooltip-autocomplete li:has-text('KITCHEN')");
    await kb.press("Enter"); // INT. KITCHEN - , and the times open at once
    await page.waitForSelector(".cm-tooltip-autocomplete li:has-text('CONTINUOUS')");
    await kb.type("cont");
    await kb.press("Enter"); // CONTINUOUS
    await kb.press("Enter");
    await kb.type("She turns off the light.");
    await saved();
    await snap("screen-1-draft.png");

    const t = await text();
    check("headings are upper-cased and completed", t.includes("INT. KITCHEN - NIGHT\n\nRain") && t.includes("INT. KITCHEN - CONTINUOUS\n\nShe"), JSON.stringify(t));
    const file = path.join(ep1, fs.readdirSync(ep1)[0]);
    const kinds = JSON.parse(execFileSync(bin, ["classify", file]).toString())[path.basename(file)].filter((k) => k !== "blank");
    const want = ["scene_heading", "direction", "cue", "dialogue", "scene_heading", "direction"];
    check("Rust parser reads the screenplay as intended", JSON.stringify(kinds) === JSON.stringify(want), JSON.stringify(kinds));
    const status = await page.locator(".status").textContent();
    check("status bar shows pages", /\d\/8|\bpp\b|pages?/.test(status), status);

    // 2b. Camera directions and transitions are suggested in action
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Enter");
    await kb.type("CLO");
    await page.waitForSelector(".cm-tooltip-autocomplete li:has-text('CLOSE ON')");
    check("camera directions are offered, marked as such", (await page.locator(".cm-tooltip-autocomplete").textContent()).includes("camera"));
    await kb.press("Enter");
    await kb.type(" the lamp. ANGLE ON");
    await kb.press("Enter"); // typed in full: a new paragraph, not "ON SCREEN"
    await kb.type("CU");
    await page.waitForSelector(".cm-tooltip-autocomplete li:has-text('CUT TO:')");
    await kb.press("Enter");
    await kb.press("Enter"); // after a transition: a new paragraph
    await kb.type("FADE O");
    await page.waitForSelector(".cm-tooltip-autocomplete li:has-text('FADE OUT.')");
    await kb.press("Enter");
    await saved();
    const tail = (await text()).split("She turns off the light.")[1];
    check("camera terms and transitions are written as intended", tail === "\n\nCLOSE ON the lamp. ANGLE ON\n\nCUT TO:\n\n> FADE OUT.", JSON.stringify(tail));
    const kinds2 = JSON.parse(execFileSync(bin, ["classify", file]).toString())[path.basename(file)].filter((k) => k !== "blank");
    check("CUT TO: and FADE OUT. read as transitions", JSON.stringify(kinds2.slice(-3)) === JSON.stringify(["direction", "transition", "transition"]), JSON.stringify(kinds2));

    // 3. Add an episode
    await page.click("text=+ Add episode");
    await page.waitForFunction(() => document.querySelectorAll(".episode-head").length === 2);
    check("episode 2 folder created", fs.existsSync(path.join(root, "episodes/02/acts/01")));
    check("project.yaml lists episode 2", /- \{number: 2, title: "Episode 2"\}/.test(fs.readFileSync(path.join(root, "project.yaml"), "utf8")));
    await page.click('section[aria-label="Episode 2, Act One"] .act-head .add');
    await page.waitForFunction(() => document.querySelector('[data-testid="meta-title"]')?.value === "New scene");
    await page.waitForFunction(() => (window.__swText?.() ?? "").startsWith("INT. "));
    check("a new screen scene starts with a heading", (await text()).startsWith("INT. "));

    // 4. Production view: breakdown
    await page.click("text=Production");
    await page.waitForSelector("table.breakdown");
    await page.selectOption('select[aria-label="Episode"]', { label: "Whole series" });
    const rows = await page.locator("table.breakdown tbody tr").allTextContents();
    check("breakdown lists both headings of scene 1", rows.filter((r) => /KITCHEN/.test(r)).length === 2, JSON.stringify(rows));
    check("breakdown marks night", (await page.locator("table.breakdown .time.night").count()) === 1);
    await snap("screen-2-production.png");

    // 5. Export (episode 1)
    await page.click('[data-testid="export"]');
    await page.selectOption(".menu label.ep:first-child select", "1");
    const [pdf] = await Promise.all([page.waitForEvent("download"), page.click(".menu button:has-text('PDF')")]);
    const pdfPath = path.join(tmp, pdf.suggestedFilename());
    await pdf.saveAs(pdfPath);
    const head = fs.readFileSync(pdfPath).subarray(0, 5).toString();
    check("PDF downloaded", head === "%PDF-", head + " " + pdf.suggestedFilename());
    let pdfText = "";
    try {
      pdfText = execFileSync("pdftotext", ["-layout", pdfPath, "-"]).toString();
    } catch {
      pdfText = null;
    }
    if (pdfText !== null) {
      check("PDF has the title page and the scene", /NIGHT SHIFT/i.test(pdfText) && /INT\. KITCHEN - NIGHT/.test(pdfText) && /You're late\./.test(pdfText), pdfText.slice(0, 400));
      check("PDF opens with FADE IN: and closes the show", /FADE IN:/.test(pdfText) && /FADE OUT\.\s+END OF SHOW/.test(pdfText), pdfText.slice(-300));
    }
    await page.click('[data-testid="export"]');
    check("export defaults to the open scene's episode", (await page.locator(".menu label.ep:first-child select").inputValue()) === "2");
    await page.selectOption(".menu label.ep:first-child select", "1");
    const [fnt] = await Promise.all([page.waitForEvent("download"), page.click(".menu button:has-text('Fountain')")]);
    const fntPath = path.join(tmp, fnt.suggestedFilename());
    await fnt.saveAs(fntPath);
    const fountain = fs.readFileSync(fntPath, "utf8");
    check("Fountain export has a title page and the heading", /^Title: Night Shift/m.test(fountain) && fountain.includes("INT. KITCHEN - NIGHT"), fountain.slice(0, 300));

    // 6. Delete an empty act, then a whole episode
    await page.click(`nav[aria-label="Views"] >> text=Script`);
    await page.click('.episode-head[data-episode="2"] .add');
    await page.waitForSelector('section[aria-label="Episode 2, Act Two"]');
    check("an act can be added to episode 2", fs.existsSync(path.join(root, "episodes/02/acts/02")));
    await page.click('[aria-label="Delete episode 2, Act Two"]');
    await page.click('[data-testid="confirm-delete"]');
    await page.waitForFunction(() => !document.querySelector('section[aria-label="Episode 2, Act Two"]'));
    check("an empty act is deleted", !fs.existsSync(path.join(root, "episodes/02/acts/02")));
    // Episode 1 holds the drafted scene; episode 2 the new one.
    await page.click('[aria-label="Delete episode 1"]');
    const ask = await page.locator('[role="alertdialog"]').textContent();
    check("deleting an episode says what happens", /move its 1 scene to the trash/.test(ask) && /Later episodes move up one/.test(ask), ask);
    await snap("screen-3-delete-episode.png");
    await page.click('[data-testid="confirm-delete"]');
    await page.waitForFunction(() => document.querySelectorAll(".episode-head").length === 1);
    check("episode 2 became episode 1", fs.existsSync(path.join(root, "episodes/01/acts/01")) && !fs.existsSync(path.join(root, "episodes/02")));
    const trash = fs.readdirSync(path.join(root, ".trash"));
    check("the deleted episode is in the trash", trash.length === 1 && /-ep01$/.test(trash[0]) && fs.readdirSync(path.join(root, ".trash", trash[0], "acts/01")).length === 1, trash.join(","));
    const yaml2 = fs.readFileSync(path.join(root, "project.yaml"), "utf8");
    check("project.yaml lists one episode, renumbered", /episodes:\n  - \{number: 1, title: "Episode 1"\}\n/.test(yaml2) && !/number: 2/.test(yaml2), yaml2);
    const left = fs.readdirSync(path.join(root, "episodes/01/acts/01"));
    check("its scene says episode 1", left.length === 1 && /\nepisode: 1\n/.test(fs.readFileSync(path.join(root, "episodes/01/acts/01", left[0]), "utf8")));
    await page.waitForFunction(() => (window.__swText?.() ?? "").startsWith("INT. "));
    check("the editor shows a remaining scene", true);
    check("the last episode can't be deleted", (await page.locator('[aria-label="Delete episode 1"]').count()) === 0);

    check("no browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    await snap("screen-error.png");
  } finally {
    await browser.close();
    server.kill();
  }
  finish();
})();
