// End-to-end check for characters and places: name suggestions in action,
// and the Cast view (unlisted names, add, edit, rename everywhere, places on
// stage and in a film).
//
//   cargo build -p stagewright && npm run build && node e2e/cast.cjs

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, serverUp } = require("./lib.cjs");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const shots = process.env.SHOTS_DIR;



async function open(browser, fixture, edit) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-cast-"));
  copyDir(path.join(repo, "fixtures", fixture), project);
  if (edit) edit(project);
  const port = 5100 + Math.floor(Math.random() * 300);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);
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
  const quay = (p) => path.join(p, "acts/01/010-the-quay.scene");
  let s1, s2;
  try {
    // A typo'd cue in the quay scene, for the Cast view to find.
    s1 = await open(browser, "stage-sample", (p) => {
      const t = fs.readFileSync(quay(p), "utf8");
      fs.writeFileSync(quay(p), t.replace("HETTIE\nTobias, the sign", "HETIE\nTobias, the sign"));
    });
    const { page, project } = s1;
    const kb = page.keyboard;
    const text = () => page.evaluate(() => window.__swText());
    const yaml = (f) => fs.readFileSync(path.join(project, f), "utf8");
    const views = (name) => page.click(`nav[aria-label="Views"] >> text=${name}`);

    // 1. Names in action
    await page.click(".cm-content");
    await kb.press("Control+End");
    await kb.press("Enter");
    await kb.type("Suddenly HETT"); // (HETIE, the planted typo, also starts with HE)
    await page.waitForSelector(".cm-tooltip-autocomplete li:has-text('HETTIE')");
    await kb.press("Enter");
    await kb.type(" waves. TOBIAS");
    await kb.press("Enter"); // a name typed in full: Enter is a new line, not a pick
    await kb.type("Then nothing.");
    const t = await text();
    check("a capitalised name in action is completed", t.includes("Suddenly HETTIE waves. TOBIAS\n\nThen nothing."), JSON.stringify(t.slice(-80)));
    await kb.press("Enter");
    await kb.type("The end");
    check("ordinary capitalised words get no list", (await page.locator(".cm-tooltip-autocomplete").count()) === 0);
    await kb.type(". CLO");
    await page.waitForTimeout(300);
    check("a stage play is not offered camera directions", (await page.locator(".cm-tooltip-autocomplete").count()) === 0);

    // 2. Cast view: the typo is flagged, and renamed onto the right name
    await views("Cast");
    await page.waitForSelector('[data-testid="unlisted-characters"]');
    check("a cue not in the list is flagged", (await page.locator('[data-testid="unlisted-characters"] .cue').allTextContents()).join() === "HETIE");
    await snap(page, "cast-1.png");
    await page.locator('[data-testid="unlisted-characters"] >> text=Rename…').click();
    await page.fill('[data-testid="rename-to"]', "hettie");
    await page.click('[data-testid="rename-preview-btn"]');
    const preview = await page.locator('[data-testid="rename-preview"]').textContent();
    check("the rename is previewed, as a merge", /Changes 1 cue in 1 scene/.test(preview) && /become one character/.test(preview), preview);
    await page.click('[data-testid="rename-apply"]');
    await page.waitForFunction(() => !document.querySelector('[data-testid="unlisted-characters"]'));
    check("the typo is gone from the script", !fs.readFileSync(quay(project), "utf8").includes("HETIE"));

    // 3. Edit a field, add a character
    const desc = page.locator('[aria-label="Description of HETTIE"]');
    await desc.fill("Fishes; sings");
    await desc.press("Enter");
    await page.waitForFunction(() => true);
    await page.waitForTimeout(400);
    check("a description saves to characters.yaml", /cue: HETTIE.*description: "Fishes; sings"/.test(yaml("characters.yaml")), yaml("characters.yaml"));
    await page.fill('[data-testid="new-cue"]', "bosun");
    await page.click("text=Add character");
    await page.waitForSelector('tr[data-cue="BOSUN"]');
    check("a new character is added", /- \{id: bosun, cue: BOSUN, name: Bosun\}/.test(yaml("characters.yaml")), yaml("characters.yaml"));

    // 4. Rename a character everywhere
    await page.click('[aria-label="Rename TOBIAS"]');
    await page.fill('[data-testid="rename-to"]', "TOBY");
    await page.click('[data-testid="rename-preview-btn"]');
    const p2 = await page.locator('[data-testid="rename-preview"]').textContent();
    check("the preview counts cues and mentions", /cues/.test(p2) && /mention/.test(p2) && /characters\.yaml/.test(p2), p2);
    await page.click('[data-testid="rename-apply"]');
    await page.waitForSelector('tr[data-cue="TOBY"]');
    const q = fs.readFileSync(quay(project), "utf8");
    check("cues and action now say TOBY", q.includes("\nTOBY\n") && q.includes("TOBY is up a ladder") && !/\bTOBIAS\b/.test(q));
    check("dialogue keeps the name as spoken", q.includes("Tobias, the sign is upside down."));
    await views("Script");
    check("the open scene shows the new name", (await text()).includes("HETTIE waves. TOBY"));
    await views("Cast");

    // 5. Places (stage): add, then remove
    await page.fill('[data-testid="new-place"]', "The harbour wall");
    await page.click("text=Add place");
    await page.waitForSelector('tr[data-place="the-harbour-wall"]');
    check("a place is added", yaml("locations.yaml").includes("- {id: the-harbour-wall, name: The harbour wall}"));
    await page.click('[aria-label="Remove place The harbour wall from the list"]');
    await page.click('[aria-label="Remove place The harbour wall from the list"]');
    await page.waitForFunction(() => !document.querySelector('tr[data-place="the-harbour-wall"]'));
    check("and removed", !yaml("locations.yaml").includes("harbour-wall"));
    check("no browser errors (stage)", s1.errors.length === 0, s1.errors.join(" | "));

    // 6. Places (film): headings are listed, added, renamed in every heading
    s2 = await open(browser, "film-sample");
    const f = s2.page;
    await f.click(`nav[aria-label="Views"] >> text=Cast`);
    await f.waitForSelector('[data-testid="unlisted-places"]');
    check("heading places are found", (await f.locator('[data-testid="unlisted-places"] .cue').allTextContents()).join() === "HARBOUR OFFICE,HARBOUR SLIPWAY,TIDAL SANDS");
    await f.click('[data-testid="add-place-HARBOUR OFFICE"]');
    await f.waitForSelector('tr[data-place="harbour-office"]');
    await f.click('[aria-label="Rename place Harbour office"]');
    await f.fill('[data-testid="rename-to"]', "Harbourmaster's office");
    await f.click('[data-testid="rename-preview-btn"]');
    const p3 = await f.locator('[data-testid="rename-preview"]').textContent();
    check("a place rename counts headings", /1 scene heading/.test(p3), p3);
    await f.click('[data-testid="rename-apply"]');
    await f.waitForSelector("text=HARBOURMASTER'S OFFICE");
    const office = fs.readFileSync(path.join(s2.project, "acts/01/020-the-harbour-office.scene"), "utf8");
    check("the heading is renamed", office.includes("INT. HARBOURMASTER'S OFFICE - CONTINUOUS"), office.slice(0, 200));
    await snap(f, "cast-2-film.png");
    check("no browser errors (film)", s2.errors.length === 0, s2.errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    if (s1) await snap(s1.page, "cast-error.png");
  } finally {
    await browser.close();
    s1?.server.kill();
    s2?.server.kill();
  }
  finish();
})();
