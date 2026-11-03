<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/state.svelte";
  import Welcome from "./components/Welcome.svelte";
  import SceneTree from "./components/SceneTree.svelte";
  import ScriptEditor from "./components/ScriptEditor.svelte";
  import MetaPanel from "./components/MetaPanel.svelte";
  import ProductionView from "./components/ProductionView.svelte";
  import CastView from "./components/CastView.svelte";
  import FileEditor from "./components/FileEditor.svelte";
  import ShareDialog from "./components/ShareDialog.svelte";
  import UpdateToast from "./components/UpdateToast.svelte";
  import { updates } from "./lib/updates.svelte";

  let sharing = $state(false);
  let exporting = $state(false);
  /** Series: export the episode of the open scene (or choose one). */
  let exportEpisode = $state<number | null>(null);

  function openExport() {
    exportEpisode = app.project?.series ? (app.scene?.summary.episode ?? app.project.episodes[0]?.number ?? null) : null;
    exporting = !exporting;
  }

  async function doExport(kind: "pdf" | "fountain") {
    exporting = false;
    await app.exportScript(kind, exportEpisode);
  }

  let ready = $state(false);

  onMount(() => {
    void app.resume().then(() => (ready = true));
    updates.start();
    // Save before the window goes away.
    const onHide = () => void app.flush();
    // Ctrl+\ (Cmd+\ on a Mac) shows or hides the scene list.
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "\\" && app.project) {
        e.preventDefault();
        app.toggleTree();
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("beforeunload", onHide);
    document.addEventListener("visibilitychange", onHide);
    return () => {
      window.removeEventListener("beforeunload", onHide);
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("visibilitychange", onHide);
    };
  });

  const statusText: Record<string, string> = {
    saved: "Saved",
    unsaved: "Edited",
    saving: "Saving…",
    conflict: "Changed on disk",
    error: "Not saved",
  };

  $effect(() => {
    const p = app.project;
    const s = app.scene;
    document.title = p ? `${s?.summary.title ? s.summary.title + " — " : ""}${p.title} · Stagewright` : "Stagewright";
  });
</script>

{#if !ready}
  <div class="loading muted">Loading…</div>
{:else if !app.project}
  <Welcome />
{:else}
  <div class="shell" class:no-meta={!app.metaOpen || app.view.kind !== "script" || !app.scene} class:no-tree={!app.treeOpen}>
    <header>
      <button
        class="ghost tree-toggle"
        onclick={() => app.toggleTree()}
        title={app.treeOpen ? "Hide the scene list (Ctrl+\\)" : "Show the scene list (Ctrl+\\)"}
        aria-label={app.treeOpen ? "Hide the scene list" : "Show the scene list"}
        aria-pressed={app.treeOpen}
        data-testid="tree-toggle"
      >
        <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" fill="none" stroke="currentColor" />
          <rect x="2" y="3" width="4" height="10" fill="currentColor" opacity={app.treeOpen ? 0.55 : 0.12} />
        </svg>
      </button>
      <div class="title">
        <strong>{app.project.title}</strong>
        {#if app.project.authors.length}<span class="muted">· {app.project.authors.join(" & ")}</span>{/if}
      </div>
      <nav aria-label="Views">
        <button class="ghost" class:on={app.view.kind === "script"} onclick={() => (app.view = { kind: "script" })}>Script</button>
        <button class="ghost" class:on={app.view.kind === "production"} onclick={() => (app.view = { kind: "production" })}>Production</button>
        <button class="ghost" class:on={app.view.kind === "cast"} onclick={() => (app.view = { kind: "cast" })}>Cast</button>
        <select
          aria-label="Project files"
          value={app.view.kind === "file" ? app.view.name : ""}
          onchange={(e) => {
            const v = (e.currentTarget as HTMLSelectElement).value;
            if (v) app.view = { kind: "file", name: v };
          }}
        >
          <option value="" disabled>Project files…</option>
          {#each app.project.files as f}<option value={f}>{f}</option>{/each}
        </select>
      </nav>
      <div class="right">
        {#if app.collab}
          <div class="presence" aria-label="Co-writers">
            {#each app.collab.peers as p (p.clientId)}
              <span class="avatar" style="background:{p.color}" title="{p.name}{p.scene ? '' : ' (no scene open)'}">{p.name.slice(0, 1).toUpperCase()}</span>
            {/each}
          </div>
          <button class="ghost live live-{app.collab.status}" onclick={() => (sharing = true)} data-testid="collab-status">
            <span class="dot"></span>{app.collab.status === "connected" ? "Live" : app.collab.status === "connecting" ? "Connecting…" : "Offline"}
          </button>
        {:else}
          <span class="save save-{app.saveStatus}" data-testid="save-status">{statusText[app.saveStatus]}</span>
          <button class="ghost" onclick={() => (sharing = true)} title="Co-write this script live">Share…</button>
        {/if}
        {#if app.view.kind === "script" && app.scene}
          <button class="ghost" onclick={() => (app.metaOpen = !app.metaOpen)} aria-pressed={app.metaOpen} title="Show or hide scene details">
            Details
          </button>
        {/if}
        <div class="menu-wrap">
          <button class="ghost" onclick={openExport} aria-expanded={exporting} data-testid="export">Export</button>
          {#if exporting}
            <div class="menu" role="menu">
              {#if app.project.series}
                <label class="ep">
                  <span>Episode</span>
                  <select bind:value={exportEpisode}>
                    {#each app.project.episodes as e}<option value={e.number}>{e.number}: {e.title}</option>{/each}
                    <option value={null}>Whole series</option>
                  </select>
                </label>
              {/if}
              <button class="ghost" role="menuitem" onclick={() => doExport("pdf")}>PDF ({app.project.format_label})</button>
              {#if app.project.layouts.length > 1}
                <label class="ep" title="How the PDF is laid out. Fountain exports are always standard.">
                  <span>Layout</span>
                  <select
                    value={app.project.layout}
                    onchange={(e) => {
                      const v = e.currentTarget.value;
                      void app.setOption("layout", v === "standard" ? null : v);
                    }}
                    data-testid="layout"
                  >
                    {#each app.project.layouts as l}<option value={l.id}>{l.name}</option>{/each}
                  </select>
                </label>
              {/if}
              <button class="ghost" role="menuitem" onclick={() => doExport("fountain")}>Fountain (.fountain)</button>
              <label class="opt" title="When a character speaks again after action, in the same scene. Shown faintly in the editor; printed in exports.">
                <input
                  type="checkbox"
                  checked={app.project.auto_contd}
                  onchange={(e) => app.setOption("auto_contd", e.currentTarget.checked)}
                  data-testid="auto-contd"
                />
                Add (CONT'D) automatically
              </label>
              {#if app.project.format !== "stage"}
                <label class="opt" title="A spec script is what you send out: no camera directions, no CUT TO:, no draft on the title page. A shooting script has them.">
                  <input
                    type="checkbox"
                    checked={app.project.shooting_script}
                    onchange={(e) => app.setOption("shooting_script", e.currentTarget.checked ? true : null)}
                    data-testid="shooting-script"
                  />
                  Shooting script
                </label>
              {/if}
            </div>
          {/if}
        </div>
        <button class="ghost" onclick={() => app.closeProject()} title="Close project">Close</button>
      </div>
    </header>

    {#if app.error}
      <div class="error" role="alert">
        <span>{app.error}</span>
        {#if app.saveStatus === "error"}<button onclick={() => app.retrySave()}>Retry save</button>{/if}
        <button class="ghost" onclick={() => (app.error = null)} aria-label="Dismiss">✕</button>
      </div>
    {/if}

    {#if app.treeOpen}<aside class="tree"><SceneTree /></aside>{/if}

    <main>
      <!-- The script editor stays alive behind other views, so switching back
           keeps the text, cursor, scroll and undo history exactly as they were. -->
      {#if app.scene}
        <div class="pane" class:gone={app.view.kind !== "script"}><ScriptEditor /></div>
      {/if}
      {#if app.view.kind === "production"}
        <ProductionView />
      {:else if app.view.kind === "cast"}
        <CastView />
      {:else if app.view.kind === "file"}
        {#key app.view.name}<FileEditor name={app.view.name} />{/key}
      {:else if !app.scene}
        <div class="empty muted">Choose a scene, or add one.</div>
      {/if}
    </main>

    {#if app.metaOpen && app.view.kind === "script" && app.scene}
      <aside class="meta"><MetaPanel /></aside>
    {/if}
  </div>
  {#if sharing}<ShareDialog onclose={() => (sharing = false)} />{/if}
  {#if app.collab?.problem}
    <div class="collab-problem" role="status">Co-writing: {app.collab.problem}</div>
  {/if}
{/if}
<UpdateToast />

<style>
  .loading {
    display: grid;
    place-items: center;
    height: 100%;
  }
  .shell {
    display: grid;
    grid-template-columns: 264px minmax(0, 1fr) 300px;
    grid-template-rows: 48px auto minmax(0, 1fr);
    grid-template-areas:
      "head head head"
      "err err err"
      "tree main meta";
    height: 100%;
  }
  .shell.no-meta {
    grid-template-columns: 264px minmax(0, 1fr) 0;
  }
  .shell.no-tree {
    grid-template-columns: 0 minmax(0, 1fr) 300px;
  }
  .shell.no-tree.no-meta {
    grid-template-columns: 0 minmax(0, 1fr) 0;
  }
  .tree-toggle {
    padding: 4px 6px;
    margin-left: -8px;
    display: flex;
    align-items: center;
    color: var(--ink-2);
  }
  header {
    grid-area: head;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }
  .title {
    display: flex;
    gap: 6px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    flex: 0 1 auto;
  }
  nav {
    display: flex;
    gap: 2px;
    align-items: center;
  }
  nav button.on {
    background: var(--paper);
    border-color: var(--line);
    font-weight: 600;
  }
  nav select {
    width: auto;
    margin-left: 6px;
    padding: 3px 6px;
    background: transparent;
  }
  .right {
    margin-left: auto;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .save {
    font-size: 12px;
    color: var(--ink-3);
    min-width: 7em;
    text-align: right;
  }
  .menu-wrap {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 30;
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.15);
    padding: 6px;
    display: grid;
    gap: 2px;
    min-width: 240px;
  }
  .menu button {
    text-align: left;
  }
  .menu .opt {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 6px 4px;
    margin-top: 4px;
    border-top: 1px solid var(--line);
    font-size: 12.5px;
    color: var(--ink-2);
  }
  .menu .ep {
    display: grid;
    gap: 4px;
    padding: 4px 6px 8px;
    font-size: 12px;
    color: var(--ink-3);
  }
  .presence {
    display: flex;
    gap: 2px;
  }
  .avatar {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    color: #fff;
    font: 600 11px/22px var(--ui-font);
    text-align: center;
    border: 2px solid var(--panel);
  }
  .live {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
  }
  .live .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ink-3);
  }
  .live-connected .dot {
    background: var(--ok);
  }
  .live-connecting .dot {
    background: var(--warn);
  }
  .live-offline {
    color: var(--bad);
  }
  .collab-problem {
    position: fixed;
    bottom: 34px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--paper);
    border: 1px solid var(--bad);
    color: var(--bad);
    padding: 6px 12px;
    border-radius: var(--radius);
    font-size: 12.5px;
    max-width: 80vw;
  }
  .save-conflict,
  .save-error {
    color: var(--bad);
    font-weight: 600;
  }
  .error {
    grid-area: err;
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 16px;
    background: var(--accent-soft);
    color: var(--bad);
    border-bottom: 1px solid var(--line);
  }
  .error span {
    flex: 1;
  }
  .tree {
    grid-area: tree;
    border-right: 1px solid var(--line);
    overflow: auto;
    background: var(--panel);
  }
  main {
    grid-area: main;
    min-height: 0;
    min-width: 0;
    background: var(--paper);
  }
  .pane {
    height: 100%;
  }
  .pane.gone {
    display: none;
  }
  .meta {
    grid-area: meta;
    border-left: 1px solid var(--line);
    overflow: auto;
    background: var(--panel);
  }
  .empty {
    display: grid;
    place-items: center;
    height: 100%;
  }
</style>
