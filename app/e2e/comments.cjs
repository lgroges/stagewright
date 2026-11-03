// End-to-end check for comments: add one on a selection, reply, follow the text
// as it's edited, resolve, reopen after a reload, and detach when the text goes.
//
//   cargo build -p stagewright && npm run build && node e2e/comments.cjs

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, until, serverUp, pickPort } = require("./lib.cjs");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const shots = process.env.SHOTS_DIR;




(async () => {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-comments-"));
  copyDir(path.join(repo, "fixtures/stage-sample"), project);
  const port = pickPort(5100, 250);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && !/status of 400/.test(m.text()) && errors.push(m.text()));
  const snap = async (n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  const yaml = () => (fs.existsSync(path.join(project, "comments.yaml")) ? fs.readFileSync(path.join(project, "comments.yaml"), "utf8") : "");
  const quote = "Charts on every wall.";
  try {
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForSelector(".cm-content");
    check("the Comment button waits for a selection", await page.locator('[data-testid="comment-add"]').isDisabled());

    // 1. Select a passage and comment on it
    await page.evaluate((q) => window.__swSelect(q), quote);
    await page.click('[data-testid="comment-add"]');
    await page.waitForSelector('[data-testid="comments-panel"]');
    await page.fill('[data-testid="comment-name"]', "Liam");
    await page.press('[data-testid="comment-name"]', "Enter");
    await page.fill('[data-testid="comment-draft"]', "Which charts? Name one.");
    await page.click('[data-testid="comment-submit"]');
    await until(() => /Which charts\? Name one\./.test(yaml()), 5000, "comments.yaml");
    const y = yaml();
    check("the comment is saved in comments.yaml", /scene: sc-0-0/.test(y) && y.includes(`quote: ${quote}`) && /author: Liam/.test(y), y);
    check("its passage is highlighted", (await page.locator(".sw-comment").first().textContent()) === quote);
    check("the scene list counts it", (await page.locator('[data-testid="scene-comments"]').first().textContent()).includes("1"));
    await snap("comments-1-added.png");

    // 2. Reply
    const thread = page.locator(".card[data-thread]").first();
    await thread.locator('textarea[aria-label="Reply"]').fill("The Channel, at least.");
    await thread.locator('button:has-text("Reply")').click();
    await until(() => /The Channel, at least\./.test(yaml()), 5000, "the reply");
    await until(async () => (await thread.locator(".msg").count()) === 2, 3000, "the reply to show");
    check("a reply is added to the thread", true);

    // 3. Edit before the passage: the highlight follows and the anchor is stored again
    await page.click(".cm-content");
    await page.keyboard.press("Control+Home");
    await page.keyboard.type("Night. ");
    // The 40 characters before it now start "ght. A cramped" (they were "A cramped…").
    await until(() => yaml().includes("ght. A cramped office"), 6000, "the anchor to move").catch((e) => {
      console.log(yaml());
      throw e;
    });
    check("the highlight follows the text", (await page.locator(".sw-comment").first().textContent()) === quote);
    check("and the new surroundings are stored", true);

    // 4. Resolve: hidden, uncounted, kept
    await thread.locator('[data-testid="comment-resolve"]').click();
    await until(() => /resolved: true/.test(yaml()), 5000, "resolved");
    await page.waitForFunction(() => !document.querySelector(".sw-comment:not(.sw-comment-hidden)"));
    check("a resolved comment's highlight is hidden", true);
    check("and the scene list no longer counts it", (await page.locator('[data-testid="scene-comments"]').count()) === 0);
    check("the toolbar counts open comments", (await page.locator('[data-testid="comments-toggle"]').textContent()).includes("0"));

    // 5. After a reload, Show resolved brings it back; Reopen reopens it
    await page.reload();
    await page.waitForSelector('[data-testid="comments-panel"]');
    await page.check('[data-testid="comments-panel"] input[type="checkbox"]');
    await page.waitForSelector(".card[data-thread]");
    await page.locator('.card[data-thread] [data-testid="comment-resolve"]').click();
    await until(() => /resolved: false/.test(yaml()), 5000, "reopened");
    await page.waitForSelector(".sw-comment:not(.sw-comment-hidden)");
    check("it survives a reload and can be reopened", (await page.locator(".sw-comment").first().textContent()) === quote);

    // 6. Delete the passage: the comment stays, detached
    await page.evaluate((q) => window.__swSelect(q), quote);
    await page.keyboard.press("Delete");
    await page.waitForSelector(".detached");
    check("deleting the text leaves the comment, marked detached", y.includes(quote) && yaml().includes(quote));
    await snap("comments-2-detached.png");

    // 7. Delete the thread
    await page.locator('.card[data-thread] button:has-text("Delete")').last().click();
    await page.locator('button:has-text("Delete thread?")').click();
    await until(() => !yaml().includes("Which charts"), 5000, "deleted");
    await until(async () => (await page.locator(".card[data-thread]").count()) === 0, 3000, "the card to go");
    check("a thread can be deleted", true);
    check("scene files never contain comments", !fs.readFileSync(path.join(project, "acts/00/000-prologue.scene"), "utf8").includes("Which charts"));
    check("no browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    await snap("comments-error.png");
  } finally {
    await browser.close();
    server.kill();
  }
  finish();
})();
