// End-to-end check for the AI review, against a stand-in for Ollama: the
// Review button, its notes marked apart from the rules', their fixes, saving
// while a review is under way, and what happens when Ollama isn't there.
//
//   cargo build -p stagewright && npm run build && node e2e/ai.cjs

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, until, serverUp, pickPort } = require("./lib.cjs");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");

const shots = process.env.SHOTS_DIR;

/** A stand-in Ollama: one model, and a review that takes `delay` ms. */
function fakeOllama(port, delay) {
  const asked = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const reply = (v) => {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify(v));
      };
      if (req.url === "/api/tags") return reply({ models: [{ name: "qwen3.6:27b" }] });
      if (req.url === "/api/chat") {
        asked.push(JSON.parse(body));
        // Each paragraph back as sent, but one corrected, and one that isn't in the scene.
        const req = JSON.parse(body);
        const paragraphs = [...req.messages[1].content.matchAll(/^\[(\d+)\] (.*)$/gm)].map(([, n, text]) => ({
          n: Number(n),
          text: text.replace("stamps papers without", "stamps the papers without"),
        }));
        paragraphs.push({ n: 999, text: "Words the model made up." });
        const content = JSON.stringify({ paragraphs });
        return setTimeout(() => reply({ message: { role: "assistant", content } }), delay);
      }
      res.writeHead(404);
      res.end();
    });
  });
  server.listen(port, "127.0.0.1");
  return { server, asked };
}

(async () => {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-ai-"));
  copyDir(path.join(repo, "fixtures/stage-sample"), project);
  const port = pickPort(5700, 250);
  const ollamaPort = pickPort(5950, 40);
  const ollama = fakeOllama(ollamaPort, 2500);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && !/status of 400/.test(m.text()) && errors.push(m.text()));
  await page.addInitScript((url) => localStorage.setItem("stagewright.ai.url", url), `http://127.0.0.1:${ollamaPort}`);
  const snap = async (n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  const text = () => page.evaluate(() => window.__swText());
  const sceneFile = () => fs.readFileSync(path.join(project, "acts/00/000-prologue.scene"), "utf8");
  const status = () => page.locator('[data-testid="ai-status"]').textContent();
  try {
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForSelector(".cm-content");

    // 1. Review: the editor keeps saving while the model thinks
    await page.click('[data-testid="ai-review"]');
    await page.waitForSelector('[data-testid="ai-status"]');
    check("Review shows it's under way", /Reviewing/.test(await status()) && (await page.locator('[data-testid="ai-review"]').isDisabled()));
    await page.click(".cm-content");
    await page.keyboard.press("Control+Home");
    await page.keyboard.type("Night. ");
    await until(() => sceneFile().includes("Night. A cramped office"), 2000, "a save during the review");
    check("the scene saves while the review runs", true);
    await page.waitForFunction(() => /1 note/.test(document.querySelector('[data-testid="ai-status"]')?.textContent ?? ""), null, { timeout: 10000 });
    check("the review reports its notes, leaving out paragraphs that aren't in the scene", /qwen3\.6:27b: 1 note/.test(await status()), await status());
    const asked = ollama.asked[0];
    check("it asked for JSON from the model, without thinking", asked?.model === "qwen3.6:27b" && asked.think === false && asked.format?.type === "object" && asked.options.num_ctx >= 8192);
    check("and gave it the cast", asked.messages[1].content.includes("MORDECAI"));

    // 2. The note is underlined apart from the rules', and follows the edit made meanwhile
    await page.waitForSelector(".sw-ai");
    check("its words are marked as an AI note", (await page.locator(".sw-ai").first().textContent()) === "stamps papers");
    await snap("ai-1-note.png");
    await page.click('[data-testid="issue-count"]');
    await page.waitForSelector(".cm-panel-lint");
    const panel = await page.locator(".cm-panel-lint").textContent();
    check("the list says it's from the AI review", panel.includes("AI review"), panel);
    await page.locator('.cm-panel-lint button:has-text("Change to")').first().click();
    await until(async () => (await text()).includes("The ADMIRAL stamps the papers without reading them."), 3000, "the fix");
    check("its fix puts the suggestion in", true);
    await page.keyboard.press("Escape");
    await until(async () => (await page.locator(".sw-ai").count()) === 0, 3000, "the note to go once fixed");
    check("and the note goes once its words have changed", true);

    // 3. Reviewing the same text again uses the kept review, without asking the model
    await page.keyboard.press("Control+z");
    await until(async () => (await text()).includes("ADMIRAL stamps papers"), 3000, "undo");
    await until(() => sceneFile().includes("ADMIRAL stamps papers"), 3000, "the undo saved");
    const review = async () => {
      await page.click('[data-testid="ai-review"]');
      await page.waitForFunction(() => /1 note/.test(document.querySelector('[data-testid="ai-status"]')?.textContent ?? ""), null, { timeout: 10000 });
    };
    await review();
    const before = ollama.asked.length;
    await review();
    check("a review of unchanged text is kept, not asked again", ollama.asked.length === before, `${ollama.asked.length} vs ${before}`);
    check("cached reviews are kept out of git", fs.existsSync(path.join(project, ".stagewright/.gitignore")));

    // 4. Ollama goes away: the review says so; the rule checks carry on
    ollama.server.close();
    await page.click(".cm-content");
    await page.keyboard.press("Control+End");
    await page.keyboard.type("\n\nThe the bell rings again.");
    await page.waitForSelector(".cm-lintRange-warning, .cm-lintRange-info:not(.sw-ai)");
    await page.click('[data-testid="ai-review"]');
    await page.waitForFunction(() => /Is it running\?/.test(document.querySelector('[data-testid="ai-status"]')?.textContent ?? ""), null, { timeout: 10000 });
    check("without Ollama, Review explains why", /isn't answering|Couldn't reach/.test(await status()), await status());
    await snap("ai-2-unavailable.png");
    check("and the rule checks still run", (await page.locator(".cm-lintRange-warning, .cm-lintRange-info:not(.sw-ai)").count()) > 0);
    await page.click('nav[aria-label="Views"] >> text=Production');
    await page.click('button:has-text("Checks…")');
    await page.waitForFunction(() => /Is it running\?/.test(document.querySelector('[data-testid="ai-reachable"]')?.textContent ?? ""), null, { timeout: 8000 });
    check("Production → Checks shows Ollama isn't running", true);
    await page.locator('[data-testid="ai-settings"]').scrollIntoViewIfNeeded();
    await snap("ai-3-settings.png");

    // 5. Switched off for the project
    await page.locator('[data-testid="script-check"] label:has-text("AI review") input').uncheck();
    await until(() => /ai-grammar/.test(fs.readFileSync(path.join(project, "project.yaml"), "utf8")), 3000, "switched off");
    await page.click('nav[aria-label="Views"] >> text=Script');
    await page.click('[data-testid="ai-review"]');
    await page.waitForFunction(() => /switched off/.test(document.querySelector('[data-testid="ai-status"]')?.textContent ?? ""), null, { timeout: 8000 });
    check("a project can switch the AI review off", true);
    check("no browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    await snap("ai-error.png");
  } finally {
    await browser.close();
    server.kill();
    ollama.server.close();
  }
  finish();
})();
