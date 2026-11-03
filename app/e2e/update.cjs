// End-to-end check for self-update, in Chromium with Tauri's bridge stubbed:
// the app's own API calls go to `stagewright serve`, and the updater and
// process plugins are fakes that record what the app asks of them. This covers
// the app's side (offer, notes, Later, manual check, save before install,
// progress, restart); the real download and signature check are Tauri's.
//
//   cargo build -p stagewright && npm run build && node e2e/update.cjs

const { chromium } = require("playwright");
const { repo, bin, check, fail, finish, copyDir, serverUp, pickPort } = require("./lib.cjs");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const port = pickPort(5050, 40);
const shots = process.env.SHOTS_DIR;
const version = JSON.parse(fs.readFileSync(path.join(__dirname, "../package.json"), "utf8")).version;


function fakeTauri() {
  const callbacks = {};
  let next = 1;
  window.__calls = [];
  window.__update = null; // what `check` returns; set by the test
  window.__checkFails = false;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback(cb) {
      const id = next++;
      callbacks[id] = cb;
      return id;
    },
    unregisterCallback(id) {
      delete callbacks[id];
    },
    convertFileSrc: (s) => s,
    async invoke(cmd, args) {
      window.__calls.push(cmd);
      if (cmd === "api") {
        const r = await fetch(`/api/${args.cmd}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(args.args),
        });
        const j = await r.json();
        if (!r.ok) throw j;
        return j;
      }
      if (cmd === "plugin:updater|check") {
        if (window.__checkFails) throw "error sending request: connection refused";
        return window.__update;
      }
      if (cmd === "plugin:updater|download_and_install") {
        const send = (index, message) => callbacks[args.onEvent.id]?.({ index, message });
        const wait = () => new Promise((r) => setTimeout(r, 250));
        send(0, { event: "Started", data: { contentLength: 1000 } });
        await wait();
        send(1, { event: "Progress", data: { chunkLength: 600 } });
        await wait();
        send(2, { event: "Progress", data: { chunkLength: 400 } });
        send(3, { event: "Finished" });
        callbacks[args.onEvent.id]?.({ index: 4, end: true });
        await wait();
        return null;
      }
      if (cmd === "plugin:process|restart") {
        window.__restarted = true;
        return new Promise(() => {}); // a real restart never returns
      }
      if (cmd === "plugin:resources|close") return null;
      throw new Error(`not faked: ${cmd}`);
    },
  };
}


(async () => {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "sw-update-"));
  copyDir(path.join(repo, "fixtures/stage-sample"), project);
  const server = spawn(bin, ["serve", project, "--port", String(port), "--static", path.join(repo, "app/dist")], { stdio: "pipe" });
  await serverUp(port);

  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.addInitScript(fakeTauri);
  await page.addInitScript(() => {
    window.__update = {
      rid: 1,
      currentVersion: "0.2.0",
      version: "0.3.0",
      date: "2026-11-20 10:00:00 +00:00",
      body: "- Character sides: a script for each actor\n- Faster PDF export",
      rawJson: {},
    };
  });
  const snap = async (n) => shots && (await page.screenshot({ path: path.join(shots, n) }));
  const toast = page.locator('[data-testid="update-toast"]');

  try {
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForSelector(".cm-content");

    // 1. Checked on launch, offered with notes
    await toast.waitFor({ timeout: 5000 });
    const text = await toast.textContent();
    check("a newer version is offered on launch", text.includes("Stagewright 0.3.0 is available") && text.includes(`you have ${version}`), text);
    await toast.locator("text=What's new").click();
    check("release notes are listed", (await toast.locator("li").count()) === 2);
    await snap("update-1-offer.png");
    await toast.locator("text=Later").click();
    check("Later puts it away", (await toast.count()) === 0);

    // 2. Manual check from the start screen
    await page.click("text=Close");
    const footer = page.locator('[data-testid="app-version"]');
    await footer.waitFor();
    check("start screen shows the version", (await footer.textContent()).includes(`Stagewright ${version}`));
    await page.evaluate(() => (window.__update = null));
    await footer.locator("text=Check for updates").click();
    await page.waitForSelector("text=is up to date");
    check("a manual check says when it is up to date", true);
    await toast.locator("text=OK").click();
    await page.evaluate(() => (window.__checkFails = true));
    await footer.locator("text=Check for updates").click();
    await page.waitForSelector("text=Couldn't check for updates");
    check("a failed check says so", (await toast.textContent()).includes("connection refused"));
    await toast.locator("text=Close").click();

    // 3. Install (after a restart of the page, which checks again on launch):
    // unsaved typing is saved first, then download, install, restart.
    await page.reload();
    await page.locator('button.link[title="' + project + '"]').click(); // closed earlier: reopen from Recent
    await page.waitForSelector(".cm-content");
    await page.click(".cm-content");
    await page.keyboard.press("Control+End");
    await page.keyboard.type(" Saved before the update.");
    await toast.locator("text=Install and restart").waitFor();
    await toast.locator("text=Install and restart").click();
    await page.waitForSelector("text=Updating to 0.3.0");
    await snap("update-2-installing.png");
    await page.waitForFunction(() => window.__restarted === true, null, { timeout: 5000 });
    check("installs, then restarts", true);
    const calls = await page.evaluate(() => window.__calls);
    const dl = calls.indexOf("plugin:updater|download_and_install");
    check("downloads before restarting", dl >= 0 && calls.indexOf("plugin:process|restart") > dl, calls.join(","));
    const saved = fs
      .readdirSync(path.join(project, "acts"), { recursive: true })
      .filter((f) => f.endsWith(".scene"))
      .some((f) => fs.readFileSync(path.join(project, "acts", f), "utf8").includes("Saved before the update."));
    check("unsaved typing is on disk before the update", saved);

    check("no browser errors", errors.length === 0, errors.join(" | "));
  } catch (e) {
    console.error(e);
    fail();
    await snap("update-error.png");
  } finally {
    await browser.close();
    server.kill();
  }
  finish();
})();
