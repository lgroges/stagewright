// End-to-end check for the automatic (CONT'D) and the collapsible scene list.
//
//   cargo build -p stagewright && npm run build && node e2e/layout.cjs

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, serverUp } = require("./lib.cjs");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const shots = process.env.SHOTS_DIR;



async function open(browser, fixture) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-layout-"));
  copyDir(path.join(repo, "fixtures", fixture), project);
  const port = 5700 + Math.floor(Math.random() * 250);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, acceptDownloads: true });
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
  let s1, s2;
  try {
    // ---- (CONT'D), in a film
    s1 = await open(browser, "film-sample");
    const { page, project } = s1;
    const kb = page.keyboard;
    await page.click(".cm-content");
    await kb.press("Control+End");
    for (const [t, plain] of [["", false], ["NELL", true], ["One more thing.", false], ["", false], ["She waits. Nothing.", false], ["", false], ["NELL", true], ["Fine.", false]]) {
      if (t) await kb.type(t);
      await kb.press(plain ? "Shift+Enter" : "Shift+Enter");
    }
    await page.waitForFunction(() => document.querySelectorAll(".sw-contd").length >= 1);
    const labelled = await page.evaluate(() => [...document.querySelectorAll(".sw-contd")].map((e) => e.closest(".cm-line").textContent));
    check("a cue after action by the same character shows (CONT'D)", labelled.at(-1) === "NELL (CONT'D)", JSON.stringify(labelled));
    check("the label isn't typed into the text", !(await page.evaluate(() => window.__swText())).endsWith("NELL (CONT'D)\nFine.\n"));
    await snap(page, "layout-1-contd.png");

    // Export prints it
    await page.click('[data-testid="export"]');
    const [dl] = await Promise.all([page.waitForEvent("download"), page.click(".menu button:has-text('Fountain')")]);
    const f = fs.readFileSync(await dl.path(), "utf8");
    check("the Fountain export has (CONT'D)", f.includes("NELL (CONT'D)\nFine."), f.slice(-120));

    // Switch it off
    await page.click('[data-testid="export"]');
    await page.uncheck('[data-testid="auto-contd"]');
    await page.waitForFunction(() => document.querySelectorAll(".sw-contd").length === 0);
    check("switching it off hides the label", true);
    check("and is saved in project.yaml", /auto_contd: false/.test(fs.readFileSync(path.join(project, "project.yaml"), "utf8")));
    await page.keyboard.press("Escape");
    check("no browser errors (film)", s1.errors.length === 0, s1.errors.join(" | "));

    // ---- Folding and hiding the scene list, in a series
    s2 = await open(browser, "tv-series-sample");
    const p = s2.page;
    const visible = (sel) => p.locator(sel).isVisible();
    const ep2act = 'section[aria-label="Episode 2, Act One"]';
    check("episode 2's act is shown", await visible(ep2act));
    await p.click('[aria-label="Hide episode 2"]');
    check("folding an episode hides its acts", !(await p.locator(ep2act).count()));
    check("and says what's inside", /1 act · 1 scenes/.test(await p.locator(".folded-note").textContent()));
    await p.click('[aria-label^="Hide the scenes of episode 1, Act Two"]').catch(() => {});
    await p.reload();
    await p.waitForSelector(".cm-content");
    check("folding is remembered", !(await p.locator(ep2act).count()));
    await p.click('text="Unfold all"'); // episode 2 is still folded
    await p.click('text="Fold all"');
    // The open scene (episode 1's first) stays visible: its act and episode unfold.
    const openScene = await p.locator(".scene.selected").count();
    check("fold all keeps the open scene visible", openScene === 1);
    await p.click('text="Unfold all"');
    check("unfold all shows everything", await visible(ep2act));
    await snap(p, "layout-2-tree.png");

    const before = (await p.locator(".cm-editor").boundingBox()).width;
    await p.click('[data-testid="tree-toggle"]');
    await p.waitForFunction(() => !document.querySelector("aside.tree"));
    const after = (await p.locator(".cm-editor").boundingBox()).width;
    check("hiding the scene list widens the editor", after > before + 200, `${before} -> ${after}`);
    await snap(p, "layout-3-hidden.png");
    await p.keyboard.press("Control+\\");
    await p.waitForSelector("aside.tree");
    check("Ctrl+\\ shows it again", true);
    check("no browser errors (series)", s2.errors.length === 0, s2.errors.join(" | "));

    // ---- Layouts: the BBC studio layout for TV, from the Export menu
    await p.click('[data-testid="export"]');
    await p.selectOption('[data-testid="layout"]', "bbc-studio");
    await p.waitForFunction(() => document.querySelector('[data-testid="layout"]')?.value === "bbc-studio");
    const [pdf] = await Promise.all([p.waitForEvent("download"), p.click(".menu button:has-text('PDF')")]);
    let text = null;
    try {
      text = execFileSync("pdftotext", ["-layout", await pdf.path(), "-"]).toString();
    } catch {
      /* no pdftotext here: skip the text checks */
    }
    if (text !== null) {
      check("studio scenes are numbered, names have colons, action is bracketed", /SC1\. INT\./.test(text) && /DANA:/.test(text) && /\[A GRANDFATHER CLOCK/.test(text), text.slice(0, 600));
      check("and the episode ends END OF EPISODE", /END OF EPISODE/.test(text));
    }
    check("the layout is saved in project.yaml", /layout: "?bbc-studio"?/.test(fs.readFileSync(path.join(s2.project, "project.yaml"), "utf8")));
  } catch (e) {
    console.error(e);
    fail();
    if (s1) await snap(s1.page, "layout-error.png");
  } finally {
    await browser.close();
    s1?.server.kill();
    s2?.server.kill();
  }
  finish();
})();
