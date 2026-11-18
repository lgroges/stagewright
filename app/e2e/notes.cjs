// End-to-end check for notes, research and beats (the context web): a page for a
// character with a [[link]], a labelled link and its backlink, a research note,
// what shows beside the script, and a beat sheet from a template with where its
// beats land and a scene made for a beat.
//
//   cargo build -p stagewright && npm run build && node e2e/notes.cjs

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, until, serverUp, pickPort } = require("./lib.cjs");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const shots = process.env.SHOTS_DIR;

(async () => {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-notes-"));
  copyDir(path.join(repo, "fixtures/stage-sample"), project);
  const port = pickPort(6300, 250);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && !/status of 400/.test(m.text()) && errors.push(m.text()));
  const snap = async (n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  const file = (name) => (fs.existsSync(path.join(project, name)) ? fs.readFileSync(path.join(project, name), "utf8") : "");
  const view = (name) => page.click(`nav[aria-label="Views"] >> text=${name}`);
  const pick = (name) => page.locator('[data-testid="notes-view"] .list button.item', { hasText: name }).first().click();
  try {
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForSelector(".cm-content");

    // 1. A character's page, with a [[link]] typed using completion
    await view("Notes");
    await page.waitForSelector('[data-testid="notes-view"]');
    await pick("Hettie");
    await page.waitForSelector('[data-testid="page-editor"] .cm-content');
    await page.click('[data-testid="page-editor"] .cm-content');
    await page.keyboard.type("Sells fish on the quay. Sweet on [[tob");
    await page.waitForSelector(".cm-tooltip-autocomplete li");
    check("[[ offers the project's records", (await page.locator(".cm-tooltip-autocomplete li").first().textContent()).includes("Tobias"));
    await page.keyboard.press("Enter");
    await page.keyboard.type(".");
    await until(() => file("characters/hettie.md").includes("[[tobias]]"), 5000, "the page saved");
    check("the page is saved as characters/hettie.md, with the link", file("characters/hettie.md") === "Sells fish on the quay. Sweet on [[tobias]].");

    // 2. A labelled link, seen from the other end
    await page.click('[data-testid="add-link"]');
    const row = page.locator('[data-testid="note-links"] .link').first();
    await row.locator("select").selectOption("tobias");
    await row.locator('input[placeholder^="label"]').fill("courts");
    await row.locator('input[placeholder^="label"]').press("Tab");
    await row.locator('input[placeholder^="the other way"]').fill("courted by");
    await row.locator('input[placeholder^="the other way"]').press("Tab");
    await until(() => /links: \[\{"back":"courted by","label":"courts","to":"tobias"\}\]/.test(file("characters/hettie.md")), 5000, "the link saved");
    check("a labelled link is saved in the page's frontmatter", true);
    await snap("notes-1-hettie.png");
    await pick("Tobias");
    await page.waitForFunction(() => /courted by/.test(document.querySelector('[data-testid="note-backlinks"]')?.textContent ?? ""), null, { timeout: 5000 });
    const back = await page.locator('[data-testid="note-backlinks"]').textContent();
    check("the other end shows it with its label the other way", back.includes("courted by") && back.includes("Hettie"), back);
    check("and the scenes he's in", back.includes("The quay"), back);

    // 3. A research note linked to Tobias
    await page.fill('[data-testid="new-research"]', "Harbour bunting");
    await page.press('[data-testid="new-research"]', "Enter");
    await page.waitForFunction(() => document.querySelector('[data-testid="note-name"]')?.textContent === "Harbour bunting");
    check("a research note is made and opened", file("research/harbour-bunting.md").includes('title: "Harbour bunting"'));
    await page.fill('textarea[placeholder^="One per line"]', "Flags of the Coast (1890)");
    await page.locator('textarea[placeholder^="One per line"]').blur();
    await page.click('[data-testid="page-editor"] .cm-content');
    await page.keyboard.type("Strung for visiting officials; see [[tobias]]");
    await page.keyboard.press("Escape");
    await page.locator('[data-testid="note-title"]').click();
    await until(() => file("research/harbour-bunting.md").includes("see [[tobias]]"), 5000, "the note saved");
    const note = file("research/harbour-bunting.md");
    check("with its sources and text", note.includes('sources: ["Flags of the Coast (1890)"]') && note.includes("Strung for visiting officials"), note);

    // 4. Beside the script: the quay scene's characters, their notes, and research via Tobias
    await page.click('[data-testid="scene-tree"] >> text=The quay').catch(async () => {
      await view("Script");
      await page.locator("text=The quay").first().click();
    });
    await view("Script");
    await page.waitForSelector('[data-testid="scene-context"]');
    await until(async () => (await page.locator('[data-testid="scene-context"]').textContent()).includes("Harbour bunting"), 5000, "the context");
    const ctx = await page.locator('[data-testid="scene-context"]').textContent();
    check("beside the script: the scene's characters with their notes", ctx.includes("Hettie") && ctx.includes("Sells fish on the quay"), ctx);
    check("its place, and research linked through a character", ctx.includes("The quay at Saltmarsh") && /Harbour bunting\s*via Tobias/.test(ctx), ctx);
    await snap("notes-2-context.png");

    // 5. Beats: a sheet from a template, a scene linked, where it lands
    await view("Beats");
    await page.click('[data-testid="template-three-act"]');
    await until(() => file("plan/beats.yaml").startsWith("template: three-act"), 5000, "the sheet");
    await page.waitForSelector('[data-testid="beat"]');
    check("a sheet starts from a template", (await page.locator('[data-testid="beat"]').count()) === 7);
    const second = page.locator('[data-testid="beat"]').nth(1);
    await second.locator('[data-testid="beat-add-scene"]').selectOption({ label: "1.1 The quay" });
    await until(() => /scenes:\s*\n\s*- sc-1-1/.test(file("plan/beats.yaml")), 5000, "the scene linked");
    await page.waitForSelector('[data-testid="beat-lands"]');
    const lands = await second.locator('[data-testid="beat-lands"]').textContent();
    check("a linked beat shows where it lands", /lands \d+%, minute \d+/.test(lands), lands);
    await snap("notes-3-beats.png");
    const scenesBefore = (await page.locator('[data-testid="scene-tree"] li, .tree li').count()) || 0;
    await page.locator('[data-testid="beat"]').nth(6).locator('[data-testid="beat-make-scene"]').click();
    await until(() => /Resolution/.test(fs.readdirSync(path.join(project, "acts/02")).join(" ")) || fs.readdirSync(path.join(project, "acts/02")).some((f) => /resolution/.test(f)), 6000, "a scene for the beat");
    check("a beat can make its own scene", scenesBefore >= 0);
    await view("Beats");
    await until(async () => (await page.locator('[data-testid="beat"]').nth(6).textContent()).includes("Resolution"), 5000, "linked");
    check("and is linked to it", /Resolution/.test(await page.locator('[data-testid="beat"]').nth(6).locator(".chip").textContent()));

    // 6. A broken link is pointed out
    await view("Notes");
    await pick("Hettie");
    await page.waitForSelector('[data-testid="page-editor"] .cm-content');
    await page.click('[data-testid="page-editor"] .cm-content');
    await page.keyboard.press("Control+End");
    await page.keyboard.type(" Not [[nobody]].");
    await page.keyboard.press("Escape");
    await page.locator('[data-testid="add-link"]').focus();
    await page.waitForSelector("text=to something that doesn't exist", { timeout: 6000 });
    check("a broken link is pointed out", true);
    check("no browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    await snap("notes-error.png");
  } finally {
    await browser.close();
    server.kill();
  }
  finish();
})();
