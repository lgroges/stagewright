<script lang="ts">
  import { app } from "../lib/state.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let relay = $state(app.relay);
  let name = $state(app.writerName);
  let code = $state<string | null>(app.inviteCode());
  let copied = $state(false);
  let confirmLeave = $state(false);

  async function start() {
    code = await app.share(relay.trim(), name.trim());
  }

  async function copy() {
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      /* the code is selectable as a fallback */
    }
  }

  async function leave() {
    if (!confirmLeave) {
      confirmLeave = true;
      return;
    }
    await app.leaveSharing();
    onclose();
  }

  const validRelay = $derived(/^wss?:\/\/[^\s/]+/.test(relay.trim()));
</script>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="share-title">
    <h2 id="share-title">{app.collab ? "Co-writing" : "Share this script"}</h2>

    {#if !app.collab}
      <p class="muted">
        Anyone with the invite code can join and edit this script live. Their copy is kept in a folder on their computer;
        yours stays in this one.
      </p>
      <form onsubmit={(e) => (e.preventDefault(), start())}>
        <label class="field"><span>Your name (shown to co-writers)</span><input type="text" bind:value={name} required /></label>
        <label class="field">
          <span>Relay address</span>
          <input type="text" bind:value={relay} placeholder="wss://relay.example.com" />
        </label>
        <p class="muted small">
          The relay passes edits between writers and keeps the shared copy while everyone is offline. For testing, run
          <code>stagewright relay</code> and use <code>ws://127.0.0.1:4444</code>.
        </p>
        <div class="row">
          <button type="button" onclick={onclose}>Cancel</button>
          <button class="primary" disabled={!name.trim() || !validRelay || app.busy}>Start sharing</button>
        </div>
      </form>
    {:else}
      <p class="status">
        <span class="dot {app.collab.status}"></span>
        {app.collab.status === "connected" ? "Connected" : app.collab.status === "connecting" ? "Connecting…" : "Offline — your edits are kept and sent when you reconnect"}
        · {app.collab.config.relay}
      </p>
      {#if code}
        <label class="field">
          <span>Invite code — send it privately; it works like a password</span>
          <textarea readonly rows="3" onfocus={(e) => (e.currentTarget as HTMLTextAreaElement).select()} data-testid="invite-code">{code}</textarea>
        </label>
        <div class="row left">
          <button class="primary" onclick={copy}>{copied ? "Copied" : "Copy invite code"}</button>
        </div>
      {/if}
      <h3>Writers here now</h3>
      <ul class="peers">
        <li><span class="swatch" style="background:{app.collab.config.color}"></span>{app.collab.config.name} (you)</li>
        {#each app.collab.peers as p (p.clientId)}
          <li><span class="swatch" style="background:{p.color}"></span>{p.name}</li>
        {/each}
      </ul>
      <div class="row">
        <button class="ghost danger" onclick={leave}>{confirmLeave ? "Stop co-writing on this computer?" : "Stop co-writing"}</button>
        <button onclick={onclose}>Done</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 50;
    padding: 16px;
  }
  .dialog {
    width: min(480px, 100%);
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 22px 24px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.25);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 18px;
  }
  h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ink-3);
    margin: 18px 0 6px;
  }
  .small {
    font-size: 12.5px;
  }
  code {
    font-size: 12px;
    background: var(--panel);
    padding: 1px 4px;
    border-radius: 4px;
  }
  textarea {
    font: 12px ui-monospace, Consolas, monospace;
    word-break: break-all;
    resize: none;
  }
  .row {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 14px;
  }
  .row.left {
    justify-content: flex-start;
    margin-top: 0;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--ink-3);
  }
  .dot.connected {
    background: var(--ok);
  }
  .dot.connecting {
    background: var(--warn);
  }
  .peers {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  .peers li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 0;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }
  .danger {
    color: var(--bad);
    margin-right: auto;
  }
</style>
