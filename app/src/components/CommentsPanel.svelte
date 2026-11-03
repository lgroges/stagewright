<script lang="ts">
  import { tick } from "svelte";
  import { app } from "../lib/state.svelte";
  import type { Comment } from "../lib/api";

  interface Shown {
    comment: Comment;
    /** Null: the passage it was on has been deleted. */
    at: number | null;
  }

  let {
    shown,
    draft,
    onselect,
    onsubmit,
    oncancel,
  }: {
    shown: Shown[];
    draft: { quote: string } | null;
    onselect: (id: string) => void;
    onsubmit: (text: string) => Promise<boolean>;
    oncancel: () => void;
  } = $props();

  let draftText = $state("");
  let draftBox = $state<HTMLTextAreaElement | null>(null);
  let replies = $state<Record<string, string>>({});
  let editing = $state<{ id: string; index: number; text: string } | null>(null);
  let confirmDelete = $state<string | null>(null);
  let nameInput = $state("");

  const now = () => new Date().toISOString();
  const needName = $derived(!app.author);

  $effect(() => {
    if (draft) {
      draftText = "";
      void tick().then(() => draftBox?.focus());
    }
  });

  // Scroll the picked comment into view.
  $effect(() => {
    const id = app.activeComment;
    if (!id) return;
    void tick().then(() => document.querySelector(`[data-thread="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" }));
  });

  function when(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    const mins = (Date.now() - d.getTime()) / 60000;
    if (mins < 1) return "just now";
    if (mins < 60) return `${Math.floor(mins)} min ago`;
    const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    if (d.toDateString() === new Date().toDateString()) return time;
    return `${d.toLocaleDateString(undefined, { day: "numeric", month: "short" })} ${time}`;
  }

  const clip = (s: string, n = 90) => (s.length > n ? s.slice(0, n - 1).trimEnd() + "…" : s).replace(/\n+/g, " / ");

  async function submitDraft() {
    if (!draftText.trim()) return;
    if (await onsubmit(draftText)) draftText = "";
  }

  async function reply(id: string) {
    const text = replies[id]?.trim();
    if (!text) return;
    if (await app.editComments({ op: "reply", id, author: app.author, at: now(), text })) replies[id] = "";
  }

  async function saveEdit() {
    if (!editing || !editing.text.trim()) return;
    if (await app.editComments({ op: "edit", id: editing.id, index: editing.index, text: editing.text, at: now() })) editing = null;
  }

  function keys(e: KeyboardEvent, send: () => void, cancel?: () => void) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      send();
    } else if (e.key === "Escape" && cancel) {
      e.preventDefault();
      cancel();
    }
  }

  const open = $derived(shown.filter((s) => !s.comment.resolved).length);
  const resolved = $derived(shown.length - open);
  const visible = $derived(shown.filter((s) => app.showResolved || !s.comment.resolved));
</script>

<aside class="comments" aria-label="Comments" data-testid="comments-panel">
  <header>
    <strong>Comments</strong>
    <span class="muted">{open} open{resolved ? ` · ${resolved} resolved` : ""}</span>
    {#if resolved}
      <label class="muted small"><input type="checkbox" bind:checked={app.showResolved} /> Show resolved</label>
    {/if}
    <button class="ghost close" onclick={() => app.toggleComments(false)} aria-label="Hide comments" title="Hide comments">✕</button>
  </header>

  {#if needName}
    <form class="card name" onsubmit={(e) => (e.preventDefault(), nameInput.trim() && app.setAuthor(nameInput))}>
      <label for="comment-name">Your name, shown on your comments</label>
      <div class="row">
        <input id="comment-name" bind:value={nameInput} placeholder="Name" data-testid="comment-name" />
        <button class="primary" disabled={!nameInput.trim()}>OK</button>
      </div>
    </form>
  {/if}

  {#if draft}
    <div class="card draft active">
      <div class="quote">“{clip(draft.quote)}”</div>
      <textarea
        bind:this={draftBox}
        bind:value={draftText}
        rows="3"
        placeholder="Add a comment…"
        onkeydown={(e) => keys(e, submitDraft, oncancel)}
        data-testid="comment-draft"
      ></textarea>
      <div class="row end">
        <button class="ghost" onclick={oncancel}>Cancel</button>
        <button class="primary" onclick={submitDraft} disabled={!draftText.trim() || needName} data-testid="comment-submit">Comment</button>
      </div>
    </div>
  {:else if !shown.length}
    <p class="muted small hint">Select some text and press <b>Comment</b> (Ctrl+Alt+M) to comment on it.</p>
  {/if}

  {#each visible as { comment: c, at } (c.id)}
    <div
      class="card"
      class:active={app.activeComment === c.id}
      class:resolved={c.resolved}
      data-thread={c.id}
      role="group"
      aria-label="Comment by {c.messages[0]?.author}"
    >
      <button class="quote ghost" onclick={() => onselect(c.id)} title={at === null ? "" : "Show in the script"} disabled={at === null}>
        {#if at === null}<span class="detached">The text this was on has been deleted:</span>{/if}
        “{clip(c.quote)}”
      </button>
      {#each c.messages as m, i}
        <div class="msg">
          <div class="who">
            <strong>{m.author}</strong>
            <span class="muted">{when(m.at)}{m.edited ? " · edited" : ""}</span>
          </div>
          {#if editing && editing.id === c.id && editing.index === i}
            <textarea bind:value={editing.text} rows="3" onkeydown={(e) => keys(e, saveEdit, () => (editing = null))}></textarea>
            <div class="row end">
              <button class="ghost" onclick={() => (editing = null)}>Cancel</button>
              <button class="primary" onclick={saveEdit}>Save</button>
            </div>
          {:else}
            <div class="text">{m.text}</div>
            {#if m.author === app.author}
              <div class="own">
                <button class="link" onclick={() => (editing = { id: c.id, index: i, text: m.text })}>Edit</button>
                {#if i > 0}
                  <button class="link" onclick={() => app.editComments({ op: "delete_message", id: c.id, index: i })}>Delete</button>
                {/if}
              </div>
            {/if}
          {/if}
        </div>
      {/each}
      <div class="foot">
        <textarea
          rows="1"
          placeholder={c.resolved ? "Reply (reopens it)…" : "Reply…"}
          bind:value={replies[c.id]}
          onfocus={() => (app.activeComment = c.id)}
          onkeydown={(e) => keys(e, () => reply(c.id))}
          aria-label="Reply"
        ></textarea>
        <div class="row">
          {#if replies[c.id]?.trim()}
            <button class="primary" onclick={() => reply(c.id)} disabled={needName}>Reply</button>
          {/if}
          <button class="ghost" onclick={() => app.editComments({ op: "resolve", id: c.id, resolved: !c.resolved })} data-testid="comment-resolve">
            {c.resolved ? "Reopen" : "Resolve"}
          </button>
          {#if confirmDelete === c.id}
            <button class="ghost bad" onclick={() => ((confirmDelete = null), app.editComments({ op: "delete", id: c.id }))}>Delete thread?</button>
            <button class="ghost" onclick={() => (confirmDelete = null)}>Keep</button>
          {:else}
            <button class="ghost subtle" onclick={() => (confirmDelete = c.id)} title="Delete this comment and its replies">Delete</button>
          {/if}
        </div>
      </div>
    </div>
  {/each}
</aside>

<style>
  .comments {
    width: 300px;
    flex: none;
    border-left: 1px solid var(--line);
    background: var(--panel);
    overflow-y: auto;
    padding: 8px 10px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    position: sticky;
    top: -8px;
    background: var(--panel);
    padding: 4px 0 6px;
    z-index: 1;
  }
  header .close {
    margin-left: auto;
    padding: 2px 6px;
  }
  .small {
    font-size: 12px;
  }
  .hint {
    margin: 4px 2px;
  }
  .card {
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .card.active {
    border-color: #e0a800;
    box-shadow: 0 0 0 1px #e0a800 inset;
  }
  .card.resolved {
    opacity: 0.7;
  }
  .quote {
    text-align: left;
    font-style: italic;
    color: var(--ink-2);
    font-size: 12px;
    padding: 0 0 0 8px;
    border-left: 3px solid #f2c94c;
    white-space: normal;
  }
  .detached {
    display: block;
    font-style: normal;
    color: var(--warn);
  }
  .who {
    display: flex;
    gap: 6px;
    align-items: baseline;
    font-size: 12px;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .own {
    display: flex;
    gap: 8px;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    font-size: 11.5px;
    color: var(--ink-3);
    cursor: pointer;
  }
  .link:hover {
    color: var(--accent);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    font: inherit;
    font-size: 13px;
  }
  .row {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-wrap: wrap;
  }
  .row.end {
    justify-content: flex-end;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
  .foot .row button {
    font-size: 12px;
    padding: 2px 8px;
  }
  .subtle {
    color: var(--ink-3);
  }
  .bad {
    color: var(--bad);
  }
  .name label {
    font-size: 12px;
    color: var(--ink-2);
  }
</style>
