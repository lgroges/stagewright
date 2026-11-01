<script lang="ts">
  import { app } from "../lib/state.svelte";
  import { updates, noteItems, APP_VERSION } from "../lib/updates.svelte";

  let showNotes = $state(false);
  const notes = $derived(noteItems(updates.available?.notes ?? ""));
  const busy = $derived(updates.phase === "downloading" || updates.phase === "installing");
  const visible = $derived(
    (updates.available && !updates.dismissed) ||
      busy ||
      (updates.manual && (updates.phase === "current" || updates.phase === "error")),
  );
</script>

{#if visible}
  <div class="toast" role="status" aria-live="polite" data-testid="update-toast">
    {#if busy}
      <p><strong>Updating to {updates.available?.version}…</strong></p>
      <div class="bar" aria-hidden="true">
        <span class:indeterminate={updates.progress === null} style="width:{(updates.progress ?? 0.35) * 100}%"></span>
      </div>
      <p class="muted small">
        {updates.phase === "installing" ? "Installing. Stagewright will restart." : "Downloading. Your work is saved."}
      </p>
    {:else if updates.available && !updates.dismissed}
      <p><strong>Stagewright {updates.available.version} is available</strong> <span class="muted">(you have {APP_VERSION})</span></p>
      {#if notes.length}
        <button class="ghost link" onclick={() => (showNotes = !showNotes)} aria-expanded={showNotes}>
          {showNotes ? "Hide" : "What's new"}
        </button>
        {#if showNotes}
          <ul class="notes">{#each notes as n}<li>{n}</li>{/each}</ul>
        {/if}
      {/if}
      <div class="row">
        <button class="ghost" onclick={() => (updates.dismissed = true)}>Later</button>
        <button class="primary" onclick={() => updates.install(() => app.flush())} data-testid="update-install">Install and restart</button>
      </div>
    {:else if updates.phase === "current"}
      <p>Stagewright {APP_VERSION} is up to date.</p>
      <div class="row"><button class="ghost" onclick={() => (updates.manual = false)}>OK</button></div>
    {:else if updates.phase === "error"}
      <p>Couldn't check for updates.</p>
      <p class="muted small">{updates.error}</p>
      <div class="row">
        <button class="ghost" onclick={() => (updates.manual = false)}>Close</button>
        <button onclick={() => updates.check(true)}>Try again</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 50;
    width: min(360px, calc(100vw - 32px));
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: 0 10px 32px rgba(0, 0, 0, 0.18);
    padding: 12px 14px;
    font-size: 13px;
  }
  p {
    margin: 0 0 6px;
  }
  .small {
    font-size: 12px;
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 8px;
  }
  .link {
    padding: 0;
    font-size: 12.5px;
    text-decoration: underline;
  }
  .notes {
    margin: 6px 0 0;
    padding-left: 18px;
    max-height: 180px;
    overflow: auto;
    color: var(--ink-2);
  }
  .notes li {
    margin-bottom: 3px;
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--panel);
    overflow: hidden;
    margin: 6px 0;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }
  .bar span.indeterminate {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
</style>
