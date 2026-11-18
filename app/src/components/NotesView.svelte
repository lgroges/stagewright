<!-- Notes: a page for each character and place, research notes, and the links
     between them and the scenes (the context web). -->
<script lang="ts">
  import { api, errorText, KIND_NAMES, type ContextRecord, type Neighbour, type PageKey, type PageLink, type PageMeta } from "../lib/api";
  import { Queue } from "../lib/queue.svelte";
  import { app } from "../lib/state.svelte";
  import PageEditor from "./PageEditor.svelte";

  const notes = app.notes;

  // Reload the index when the project changes (an edit here, or a co-writer's).
  $effect(() => {
    void app.project;
    void notes.load();
  });

  let filter = $state("");
  let newTitle = $state("");
  const records = $derived(notes.index?.records ?? []);
  const linkable = $derived([...records].sort((a, b) => a.name.localeCompare(b.name)));
  const shown = (kind: ContextRecord["kind"]) =>
    records
      .filter((r) => r.kind === kind && (!filter || r.name.toLowerCase().includes(filter.toLowerCase())))
      .sort((a, b) => a.name.localeCompare(b.name));
  const groups = $derived([
    { title: "Characters", items: shown("character") },
    { title: "Places", items: shown("location") },
    { title: "Research", items: shown("research") },
  ]);

  // ------------------------------------------------------------ the open record

  const selected = $derived(notes.selected ? notes.record(notes.selected) : undefined);
  /** The page as on disk, its version, and its parts. */
  let page = $state<{ id: string; file: string; text: string; version: string } | null>(null);
  let meta = $state<PageMeta>({});
  let body = $state("");
  let links = $state<Neighbour[]>([]);
  let loading = $state(false);

  async function loadRecord(id: string) {
    loading = true;
    links = [];
    try {
      const r = await api.contextRecord(id);
      const f = r.file ? await notes.read(r.file) : null;
      const parts = f ? await api.pageParse(f.text) : null;
      if (notes.selected !== id) return; // another record was picked meanwhile
      links = r.links;
      if (r.file && f && parts) {
        page = { id, file: r.file, text: f.text, version: f.version };
        meta = parts.meta;
        body = parts.body;
      } else {
        page = null;
      }
    } catch (e) {
      notes.error = errorText(e);
    } finally {
      loading = false;
    }
  }

  // Load when another record is picked (not after our own saves).
  let loadedId = $state<string | null>(null);
  $effect(() => {
    const id = notes.selected;
    if (id && id !== loadedId && notes.index) {
      loadedId = id;
      void loadRecord(id);
    }
  });

  /**
   * Change one part of the page and save it. Saves go one after another, each
   * from the text and version the last one left.
   */
  const saves = new Queue();
  function set(key: PageKey, value: unknown) {
    void saves.run(() => apply(key, value));
  }
  async function apply(key: PageKey, value: unknown) {
    if (!page) return;
    const p = page;
    const { text } = await api.pageSet(p.text, key, value);
    if (text === p.text) return;
    const version = await notes.save(p.file, text, p.version);
    if (version === null || page?.id !== p.id) return;
    page = { ...p, text, version };
    if (key === "links" || key === "body") {
      const r = await api.contextRecord(p.id);
      if (page?.id === p.id) links = r.links; // still the same record
    }
  }

  /** "a, b" or one per line → a list; empty → null (the key is removed). */
  const list = (s: string, by = /[,\n]/) => {
    const items = s.split(by).map((x) => x.trim()).filter(Boolean);
    return items.length ? items : null;
  };

  // Labelled links: edited as a list, saved whole.
  function editLink(i: number, patch: Partial<PageLink>) {
    const next = (meta.links ?? []).map((l, k) => (k === i ? { ...l, ...patch } : l));
    for (const l of next) {
      if (!l.label?.trim()) delete l.label;
      if (!l.back?.trim()) delete l.back;
    }
    meta = { ...meta, links: next };
    if (next.every((l) => l.to)) set("links", next.length ? next : null);
  }
  function addLink() {
    meta = { ...meta, links: [...(meta.links ?? []), { to: "" }] };
  }
  function removeLink(i: number) {
    const next = (meta.links ?? []).filter((_, k) => k !== i);
    meta = { ...meta, links: next };
    set("links", next.length ? next : null);
  }

  const back = $derived(links.filter((l) => l.incoming || l.how !== "link"));

  async function createNote() {
    await notes.newResearch(newTitle);
    newTitle = "";
  }
</script>

<div class="notes" data-testid="notes-view">
  <aside class="list">
    <input type="text" class="filter" placeholder="Find…" bind:value={filter} aria-label="Find a note" />
    {#each groups as g (g.title)}
      <h4>{g.title}</h4>
      <ul>
        {#each g.items as r (r.id)}
          <li>
            <button class="ghost item" class:on={notes.selected === r.id} onclick={() => (notes.selected = r.id)}>
              {r.name}{#if r.has_page && r.kind !== "research"}<span class="dot" title="Has notes">•</span>{/if}
            </button>
          </li>
        {:else}
          <li class="muted small none">{g.title === "Research" ? "No research notes yet." : "None."}</li>
        {/each}
      </ul>
    {/each}
    <form class="new" onsubmit={(e) => (e.preventDefault(), void createNote())}>
      <input type="text" placeholder="New research note…" bind:value={newTitle} data-testid="new-research" />
      <button class="ghost" disabled={!newTitle.trim()}>Add</button>
    </form>
  </aside>

  <section class="page">
    {#if notes.error}<p class="bad small" role="alert">{notes.error}</p>{/if}
    {#if notes.index?.broken.length}
      <p class="warn small">
        {notes.index.broken.length === 1 ? "A link points" : `${notes.index.broken.length} links point`} to something that
        doesn't exist: {notes.index.broken.map((b) => `${notes.record(b.from)?.name ?? b.from} → ${b.to}`).join(", ")}.
      </p>
    {/if}
    {#if !selected}
      <div class="empty muted">
        <p>Pick a character, place or research note, or add a research note.</p>
        <p class="small">
          Notes are linked to each other and to the scenes: type <code>[[</code> in a page to link to anything, or add a
          labelled link (“scryer to” Dee). Everything linked to the scene you're writing shows beside the script.
        </p>
      </div>
    {:else}
      <header>
        <span class="kind">{KIND_NAMES[selected.kind]}</span>
        <h2 data-testid="note-name">{selected.name}</h2>
        {#if selected.kind === "research" && page}
          <button class="ghost small danger" onclick={() => page && notes.trash(page.file)}>Delete note</button>
        {/if}
      </header>
      {#if loading && !page}
        <p class="muted">Loading…</p>
      {:else if page}
        {#key page.id}
          {#if selected.kind === "research"}
            <div class="fields">
              <label>
                Title
                <input type="text" value={meta.title ?? ""} onchange={(e) => set("title", e.currentTarget.value.trim() || null)} data-testid="note-title" />
              </label>
              <label>
                Tags
                <input
                  type="text"
                  value={(meta.tags ?? []).join(", ")}
                  placeholder="comma, separated"
                  onchange={(e) => set("tags", list(e.currentTarget.value))}
                />
              </label>
              <label class="wide">
                Sources
                <textarea
                  rows="2"
                  placeholder="One per line: a book, an archive, a web page"
                  value={(meta.sources ?? []).join("\n")}
                  onchange={(e) => set("sources", list(e.currentTarget.value, /\n/))}
                ></textarea>
              </label>
            </div>
          {/if}

          <h4>Links</h4>
          <div class="links" data-testid="note-links">
            {#each meta.links ?? [] as l, i (i)}
              <div class="link">
                <span class="muted">{selected.name} —</span>
                <input type="text" placeholder="label (e.g. scryer to)" value={l.label ?? ""} onchange={(e) => editLink(i, { label: e.currentTarget.value })} />
                <span class="muted">—</span>
                <select value={l.to} onchange={(e) => editLink(i, { to: e.currentTarget.value })} aria-label="Links to">
                  <option value="" disabled>choose…</option>
                  {#each linkable.filter((r) => r.id !== selected.id) as r (r.id)}
                    <option value={r.id}>{r.label ? `${r.label} ` : ""}{r.name}</option>
                  {/each}
                </select>
                <input
                  type="text"
                  placeholder="the other way (e.g. employs)"
                  value={l.back ?? ""}
                  onchange={(e) => editLink(i, { back: e.currentTarget.value })}
                />
                <button class="ghost small" aria-label="Remove link" onclick={() => removeLink(i)}>✕</button>
              </div>
            {/each}
            <button class="ghost small" onclick={addLink} data-testid="add-link">+ Add a link</button>
          </div>

          <h4>Notes</h4>
          <PageEditor
            text={body}
            records={linkable}
            placeholder={selected.kind === "research" ? "What you found, and where. [[ links to a character, place or scene." : "Background, voice, arc… [[ links to anything."}
            onchange={(b) => set("body", b)}
          />
        {/key}
      {/if}

      <h4>Linked from</h4>
      {#if back.length}
        <ul class="back" data-testid="note-backlinks">
          {#each back as l (l.id)}
            <li>
              {#if l.label}<span class="muted">{l.label}</span>{/if}
              <button class="ghost link" onclick={() => notes.show(l)}>{l.name}</button>
              <span class="muted small">{KIND_NAMES[l.kind].toLowerCase()}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="muted small">Nothing yet.</p>
      {/if}
    {/if}
  </section>
</div>

<style>
  .notes {
    display: grid;
    grid-template-columns: 240px 1fr;
    height: 100%;
    min-height: 0;
  }
  .list {
    border-right: 1px solid var(--line);
    overflow: auto;
    padding: 12px 10px;
    background: var(--panel);
  }
  .filter {
    width: 100%;
    margin-bottom: 6px;
  }
  h4 {
    margin: 14px 0 4px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ink-3);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .item {
    width: 100%;
    text-align: left;
    padding: 3px 6px;
    font-size: 13px;
  }
  .item.on {
    background: var(--accent-soft);
  }
  .dot {
    color: var(--accent);
    margin-left: 4px;
  }
  .none {
    padding: 2px 6px;
  }
  .new {
    display: flex;
    gap: 4px;
    margin-top: 14px;
  }
  .new input {
    flex: 1;
    min-width: 0;
  }
  .page {
    overflow: auto;
    padding: 20px 28px 40px;
    max-width: 860px;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  header h2 {
    margin: 0;
    font-size: 22px;
  }
  .kind {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ink-3);
  }
  header .danger {
    margin-left: auto;
    color: var(--bad);
  }
  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px 14px;
    margin-top: 12px;
  }
  .fields label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
    color: var(--ink-2);
  }
  .fields .wide {
    grid-column: 1 / -1;
  }
  .links {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
  }
  .link {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    font-size: 13px;
  }
  .link input {
    width: 170px;
  }
  .link select {
    width: auto;
    max-width: 240px;
  }
  .back li {
    display: flex;
    gap: 6px;
    align-items: baseline;
    padding: 2px 0;
    font-size: 13px;
  }
  .empty {
    max-width: 520px;
    margin-top: 40px;
  }
  .warn {
    color: var(--warn);
  }
  .bad {
    color: var(--bad);
  }
</style>
