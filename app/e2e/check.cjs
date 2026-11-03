// End-to-end check for the script check: underlines as you write, fixes,
// "Don't check for this", the list in Production, and film rules.
//
//   cargo build -p stagewright && npm run build && node e2e/check.cjs

const { chromium } = require("playwright");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const repo = path.resolve(__dirname, "../..");
const bin = path.join(repo, "target/debug/stagewright" + (process.platform === "win32" ? ".exe" : ""));
const shots = process.env.SHOTS_DIR;

let failures = 0;
function check(name, ok, detail = "") {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : " — " + detail}`);
  if (!ok) failures++;
}

function copyDir(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const e of fs.readdirSync(from, { withFileTypes: true })) {
    const a = path.join(from, e.name), b = path.join(to, e.name);
    e.isDirectory() ? copyDir(a, b) : fs.copyFileSync(a, b);
  }
}

async function open(browser, fixture) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-check-"));
  copyDir(path.join(repo, "fixtures", fixture), project);
  const port = 5400 + Math.floor(Math.random() * 300);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await new Promise((r) => setTimeout(r, 800));
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && !/status of 400/.test(m.text()) && errors.push(m.text()));
  await page.goto(`http://127.0.0.1:${port}/`);
  await page.waitForSelector(".cm-content");
  return { project, server, page, errors };
}

(async () => {
  const browser = await chromium.launch();
  const snap = async (page, n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  let s1, s2, s3;
  try {
    s1 = await open(browser, "stage-sample");
    const { page, project } = s1;
    const kb = page.keyboard;
    const text = () => page.evaluate(() => window.__swText());
    const count = () => page.locator('[data-testid="issue-count"]').textContent().catch(() => "");
    const views = (name) => page.click(`nav[aria-label="Views"] >> text=${name}`);
    const fixInPanel = async (label) => {
      await page.click('[data-testid="issue-count"]');
      await page.waitForSelector(".cm-panel-lint");
      await page.locator(`.cm-panel-lint button:has-text("${label}")`).first().click();
    };
    const waitCount = (re) => page.waitForFunction((src) => new RegExp(src).test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""), re.source, { timeout: 5000 });
    const waitClean = () => page.waitForFunction(() => !document.querySelector('[data-testid="issue-count"]'), null, { timeout: 5000 });

    await page.click("text=The quay");
    await page.waitForFunction(() => window.__swText().includes("upside down"));
    check("a clean scene shows no issues", (await count()) === "");

    // 1. A cue in mixed case: warned, underlined, fixed
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Enter");
    await kb.type("Hettie");
    await kb.press("Shift+Enter"); // a plain new line, as someone typing without the cue keys would
    await kb.type("Where is everyone?");
    await waitCount(/1 warning/);
    check("a mixed-case cue is a warning", /⚠ 1 warning/.test(await count()), await count());
    check("and is underlined", (await page.locator(".cm-lintRange-warning").count()) === 1);
    await snap(page, "check-1-warning.png");
    await fixInPanel("Write HETTIE");
    await waitClean();
    check("the fix writes the cue in capitals", (await text()).endsWith("\n\nHETTIE\nWhere is everyone?"), JSON.stringify((await text()).slice(-40)));
    await page.keyboard.press("Escape");

    // 2. A typo'd cue: "did you mean"
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Enter");
    await kb.press("Enter");
    await kb.type("HETIE");
    await kb.press("Shift+Enter");
    await kb.type("Over here.");
    await waitCount(/1 warning/);
    await page.click('[data-testid="issue-count"]');
    const panel = await page.locator(".cm-panel-lint").textContent();
    check("a typo'd cue suggests the right name", panel.includes("HETIE isn't in the cast list. Did you mean HETTIE?"), panel);
    await page.locator('.cm-panel-lint button:has-text("Change to HETTIE")').first().click();
    await waitClean();
    check("the fix renames it", (await text()).endsWith("\n\nHETTIE\nOver here."));
    await page.keyboard.press("Escape");

    // 3. A note, switched off for the project, then back on
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Enter");
    await kb.press("Enter");
    await kb.type("TOBIAS (CONT'D)");
    await kb.press("Shift+Enter");
    await kb.type("Again.");
    await waitCount(/1 note/);
    await fixInPanel("Don't check for this");
    await waitClean();
    const yaml = () => fs.readFileSync(path.join(project, "project.yaml"), "utf8");
    check("switching a check off is saved in project.yaml", /checks: \{"off":\["contd-mismatch"\]\}/.test(yaml()), yaml());
    await page.keyboard.press("Escape");

    // 4. Production lists issues for the whole play; settings turn checks back on
    await views("Production");
    await page.waitForSelector('[data-testid="script-check"]');
    const list = (await page.locator('[data-testid="script-check"]').textContent()).replace(/\s+/g, " ");
    check("Production lists the play's notes", /Lyrics outside a song/.test(list) && /0 warnings, 1 note/.test(list), list.slice(0, 300));
    await page.click("text=Checks…");
    await page.locator("label", { hasText: "(CONT'D) after a different speaker" }).locator("input").check();
    await page.waitForFunction(() => /2 notes/.test(document.querySelector('[data-testid="script-check"] h3')?.textContent ?? ""), null, { timeout: 5000 });
    check("turning it back on removes the setting", !/checks:/.test(yaml()), yaml());
    await snap(page, "check-2-production.png");
    // Clicking an issue shows its line in the script
    await page.locator('[data-testid="script-check"] button.link', { hasText: "Lyrics outside a song" }).click();
    await page.waitForFunction(() => document.querySelector(".cm-activeLine")?.textContent?.includes("Lanterns on the water"), null, { timeout: 5000 });
    check("an issue in the list opens its line", true);
    check("no browser errors (stage)", s1.errors.length === 0, s1.errors.join(" | "));

    // 5. Film rules: heading spelling, INT without a full stop, introductions
    s2 = await open(browser, "film-sample");
    const f = s2.page;
    await f.click(".cm-content");
    await f.keyboard.press("Control+End");
    await f.keyboard.press("Enter");
    await f.keyboard.press("Enter");
    await f.keyboard.type("int kitchen - nite");
    await f.keyboard.press("Enter");
    await f.keyboard.type("Tom eats.");
    await f.waitForFunction(() => /2 notes/.test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""), null, { timeout: 5000 });
    await f.click('[data-testid="issue-count"]');
    const fp = await f.locator(".cm-panel-lint").textContent();
    check("film: misspelt time and INT. without a full stop", /NITE: did you mean NIGHT\?/.test(fp) && /Write INT\. with a full stop/.test(fp), fp);
    await f.locator('.cm-panel-lint button:has-text("Change to NIGHT")').first().click();
    await f.waitForFunction(() => /1 note/.test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""));
    await f.locator('.cm-panel-lint button:has-text("Write INT.")').first().click();
    await f.waitForFunction(() => !document.querySelector('[data-testid="issue-count"]'), null, { timeout: 5000 });
    const ft = await f.evaluate(() => window.__swText());
    check("both fixed", ft.includes("\n\nINT. KITCHEN - NIGHT\n\nTom eats."), JSON.stringify(ft.slice(-60)));
    check("TOM was introduced in an earlier scene, so no note", true);
    // Spec-script rules: (V.O.) on its own line, a camera direction; then the shooting-script switch.
    await f.click(".cm-content");
    await f.keyboard.press("Control+End");
    for (const t of ["", "", "TOM", "(V.O.)", "Not now.", "", "CLOSE ON the kettle."]) {
      if (t) await f.keyboard.type(t);
      else await f.keyboard.press("Shift+Enter");
      if (t && !t.startsWith("CLOSE")) await f.keyboard.press("Shift+Enter");
    }
    await f.keyboard.press("Escape");
    await f.waitForFunction(() => /2 notes/.test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""), null, { timeout: 5000 });
    await f.click('[data-testid="issue-count"]');
    const sp = await f.locator(".cm-panel-lint").textContent();
    check("spec script: (V.O.) and a camera direction are noted", /goes next to the character's name/.test(sp) && /CLOSE ON is a camera direction/.test(sp), sp);
    await f.locator('.cm-panel-lint button:has-text("Move (V.O.) next to the name")').first().click();
    await f.waitForFunction(() => window.__swText().includes("TOM (V.O.)\nNot now."), null, { timeout: 5000 });
    check("the fix moves (V.O.) next to the name", true);
    await f.keyboard.press("Escape");
    await f.click('[data-testid="export"]');
    await f.check('[data-testid="shooting-script"]');
    await f.waitForFunction(() => !document.querySelector('[data-testid="issue-count"]'), null, { timeout: 5000 });
    check("a shooting script allows camera directions", /shooting_script: true/.test(fs.readFileSync(path.join(s2.project, "project.yaml"), "utf8")));
    await f.keyboard.press("Escape");
    // Grammar in the action, fixed in one click
    await f.click(".cm-content");
    await f.keyboard.press("Control+End");
    await f.keyboard.press("Shift+Enter");
    await f.keyboard.press("Shift+Enter");
    await f.keyboard.type("The kettle has been stood there for hours.");
    await f.keyboard.press("Escape");
    await f.waitForFunction(() => /note/.test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""), null, { timeout: 8000 });
    await f.click('[data-testid="issue-count"]');
    const gp = await f.locator(".cm-panel-lint").textContent();
    check("grammar in the action is noted", /is colloquial/.test(gp), gp);
    await f.locator(".cm-panel-lint button", { hasText: 'Write "standing"' }).first().click();
    await f.waitForFunction(() => window.__swText().endsWith("has been standing there for hours."), null, { timeout: 5000 });
    check("and fixed", true);
    await f.keyboard.press("Escape");
    await snap(f, "check-3-film.png");
    check("no browser errors (film)", s2.errors.length === 0, s2.errors.join(" | "));

    // 6. TV: short names, introductions, headings, spelling
    s3 = await open(browser, "tv-series-sample");
    const t = s3.page;
    const tk = t.keyboard;
    const ttext = () => t.evaluate(() => window.__swText());
    await t.click(`nav[aria-label="Views"] >> text=Cast`);
    const short = t.locator('[aria-label="Short name of DANA"]');
    await short.fill("Dana");
    await short.press("Enter");
    await t.waitForFunction(() => true);
    await t.waitForTimeout(400);
    check("a short name is saved", /cue: DANA, name: "?Dana Okafor"?, short: Dana/.test(fs.readFileSync(path.join(s3.project, "characters.yaml"), "utf8")));
    await t.click(`nav[aria-label="Views"] >> text=Script`);
    await t.click(".cm-content");
    await tk.press("Control+End");
    await tk.press("Enter");
    await tk.type("Later, DANA checks her watch.");
    await t.waitForFunction(() => /1 note/.test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""), null, { timeout: 5000 });
    await t.click('[data-testid="issue-count"]');
    const tp = await t.locator(".cm-panel-lint").textContent();
    check("capitals after the introduction are noted", /DANA has already been introduced: from here on, write Dana\./.test(tp), tp);
    await t.locator('.cm-panel-lint button:has-text("Write Dana")').first().click();
    await t.waitForFunction(() => window.__swText().endsWith("Later, Dana checks her watch."));
    check("the fix writes the short name", true);
    await t.keyboard.press("Escape");
    await t.click(".cm-content");
    await tk.press("Control+End");
    await tk.type(" Da");
    await t.waitForSelector(".cm-tooltip-autocomplete li:has-text('Dana')");
    await tk.press("Enter");
    check("short names are suggested in the action", (await ttext()).endsWith("watch. Dana"));
    // A heading with description and no blank line after it
    await tk.press("Enter");
    await tk.press("Enter");
    await tk.type("EXT. YARD - NIGHT. RAIN FALLS.");
    await tk.press("Shift+Enter");
    await tk.type("Mud everywhere.");
    await t.waitForFunction(() => /1 warning/.test(document.querySelector('[data-testid="issue-count"]')?.textContent ?? ""), null, { timeout: 5000 });
    await t.click('[data-testid="issue-count"]');
    await t.locator('.cm-panel-lint button:has-text("into the action")').first().click();
    await t.waitForFunction(() => window.__swText().endsWith("EXT. YARD - NIGHT\n\nRain falls.\n\nMud everywhere."), null, { timeout: 5000 });
    check("description moves out of the heading, with blank lines", true);
    await t.keyboard.press("Escape");
    await snap(t, "check-4-tv.png");
    // Spelling
    check("spelling is checked by default", (await t.locator(".cm-content").getAttribute("spellcheck")) === "true");
    await t.click('[data-testid="spellcheck"]');
    check("and can be switched off", (await t.locator(".cm-content").getAttribute("spellcheck")) === "false");
    check("no browser errors (tv)", s3.errors.length === 0, s3.errors.join(" | "));
  } catch (e) {
    console.error(e);
    failures++;
    if (s1) await snap(s1.page, "check-error.png");
  } finally {
    await browser.close();
    s1?.server.kill();
    s2?.server.kill();
    s3?.server.kill();
  }
  console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
  process.exit(failures ? 1 : 0);
})();
