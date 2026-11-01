<script lang="ts">
  import { app } from "../lib/state.svelte";
  import { FORMATS, isTauri, type Format } from "../lib/api";
  import { updates, APP_VERSION } from "../lib/updates.svelte";

  let path = $state("");
  let creating = $state(false);
  let newTitle = $state("");
  let newRoot = $state("");
  let newFormat = $state<Format>("stage");
  let newSeries = $state(true);
  let joining = $state(false);
  let invite = $state("");
  let joinName = $state(app.writerName);
  let joinRoot = $state("");

  async function chooseJoinRoot() {
    const p = await pickFolder("Choose an empty folder for your copy of the project");
    if (p) joinRoot = p;
  }

  async function join() {
    if (invite.trim() && joinName.trim() && joinRoot.trim()) await app.join(invite.trim(), joinRoot.trim(), joinName.trim());
  }

  async function pickFolder(title: string): Promise<string | null> {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: true, multiple: false, title });
    return typeof r === "string" ? r : null;
  }

  async function openFolder() {
    if (isTauri()) {
      const p = await pickFolder("Open a Stagewright project folder");
      if (p) await app.openProject(p);
    } else if (path.trim()) {
      await app.openProject(path.trim());
    }
  }

  async function chooseNewRoot() {
    const p = await pickFolder("Choose an empty folder for the new project");
    if (p) newRoot = p;
  }

  async function create() {
    if (newRoot.trim() && newTitle.trim()) {
      const tv = newFormat === "tv-single" || newFormat === "tv-multi";
      await app.createProject(newRoot.trim(), newTitle.trim(), newFormat, tv && newSeries);
    }
  }
</script>

<div class="welcome">
  <div class="card">
    <h1>Stagewright</h1>
    <p class="muted">Scene-based writing for plays, musicals, film and television.</p>

    {#if app.error}<p class="err" role="alert">{app.error}</p>{/if}

    {#if joining}
      <form onsubmit={(e) => (e.preventDefault(), join())}>
        <label class="field">
          <span>Invite code</span>
          <textarea rows="3" bind:value={invite} placeholder="sw1.…" data-testid="join-code"></textarea>
        </label>
        <label class="field"><span>Your name (shown to co-writers)</span><input type="text" bind:value={joinName} /></label>
        <label class="field">
          <span>Folder for your copy (empty or new)</span>
          {#if isTauri()}
            <div class="row">
              <input type="text" value={joinRoot} readonly placeholder="Choose a folder" />
              <button type="button" onclick={chooseJoinRoot}>Choose…</button>
            </div>
          {:else}
            <input type="text" bind:value={joinRoot} placeholder="/path/to/my-copy" data-testid="join-root" />
          {/if}
        </label>
        <div class="row">
          <button type="button" onclick={() => (joining = false)}>Back</button>
          <button class="primary" disabled={!invite.trim() || !joinName.trim() || !joinRoot.trim() || app.busy}>
            {app.busy ? "Joining…" : "Join"}
          </button>
        </div>
      </form>
    {:else if !creating}
      <section>
        {#if isTauri()}
          <button class="primary" onclick={openFolder} disabled={app.busy}>Open project folder…</button>
        {:else}
          <form onsubmit={(e) => (e.preventDefault(), openFolder())} class="row">
            <input type="text" bind:value={path} placeholder="/path/to/project" aria-label="Project folder path" />
            <button class="primary" disabled={!path.trim() || app.busy}>Open</button>
          </form>
        {/if}
        <button onclick={() => (creating = true)}>New project…</button>
        <button onclick={() => (joining = true)}>Join a shared project…</button>
      </section>

      {#if app.recent.length}
        <section>
          <h2>Recent</h2>
          <ul>
            {#each app.recent as r}
              <li>
                <button class="ghost link" onclick={() => app.openProject(r)} title={r}>{r}</button>
                <button class="ghost x" onclick={() => app.forget(r)} aria-label="Remove from recent">✕</button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {:else}
      <form onsubmit={(e) => (e.preventDefault(), create())}>
        <label class="field"><span>Title</span><input type="text" bind:value={newTitle} placeholder="Untitled" data-testid="new-title" /></label>
        <label class="field">
          <span>Format</span>
          <select bind:value={newFormat} data-testid="new-format">
            {#each FORMATS as f}<option value={f.id}>{f.label}</option>{/each}
          </select>
        </label>
        {#if newFormat === "tv-single" || newFormat === "tv-multi"}
          <label class="check">
            <input type="checkbox" bind:checked={newSeries} />
            A series: episodes in one project, sharing characters and locations
          </label>
        {/if}
        <label class="field">
          <span>Folder (empty or new)</span>
          {#if isTauri()}
            <div class="row">
              <input type="text" value={newRoot} readonly placeholder="Choose a folder" />
              <button type="button" onclick={chooseNewRoot}>Choose…</button>
            </div>
          {:else}
            <input type="text" bind:value={newRoot} placeholder="/path/to/new-project" data-testid="new-root" />
          {/if}
        </label>
        <div class="row">
          <button type="button" onclick={() => (creating = false)}>Back</button>
          <button class="primary" disabled={!newTitle.trim() || !newRoot.trim() || app.busy}>Create</button>
        </div>
      </form>
    {/if}
    <footer class="muted small" data-testid="app-version">
      Stagewright {APP_VERSION}
      {#if updates.supported}
        ·
        <button class="ghost link" onclick={() => updates.check(true)} disabled={updates.phase === "checking"}>
          {updates.phase === "checking" ? "Checking…" : "Check for updates"}
        </button>
      {/if}
    </footer>
  </div>
</div>

<style>
  footer {
    margin-top: 24px;
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  footer .link {
    padding: 0;
    font-size: 12px;
    text-decoration: underline;
  }
  .welcome {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 16px;
  }
  .card {
    width: min(460px, 100%);
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 28px;
  }
  h1 {
    font: 700 28px var(--script-font);
    margin: 0 0 4px;
    letter-spacing: 0.02em;
  }
  h2 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ink-3);
    margin: 0 0 6px;
  }
  section {
    margin-top: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }
  .row {
    display: flex;
    gap: 8px;
    width: 100%;
  }
  form .row {
    justify-content: flex-end;
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    width: 100%;
  }
  li {
    display: flex;
    align-items: center;
  }
  .link {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
  }
  .x {
    color: var(--ink-3);
  }
  .err {
    color: var(--bad);
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 13px;
    margin: -4px 0 14px;
  }
  .check input {
    margin-top: 3px;
  }
</style>
