<script lang="ts">
  import { api, type CastView, type RenameResult } from "../lib/api";
  import { app } from "../lib/state.svelte";

  let cast = $state<CastView | null>(null);
  let err = $state<string | null>(null);

  async function load() {
    try {
      cast = await api.castView();
      err = null;
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    }
  }

  // Reload whenever the project is refreshed (after an edit here, or a change
  // from a co-writer).
  $effect(() => {
    void app.project;
    void load();
  });

  const screen = $derived(cast?.screen ?? false);
  const trackChoices = $derived([...new Set([...(cast?.tracks ?? []), "any-free"])]);

  // ------------------------------------------------------------ editing in place

  /** Save one field of a list entry when it changes (on blur / Enter). */
  async function saveField(file: "characters.yaml" | "locations.yaml", id: string, key: string, value: string, before: unknown) {
    if ((before ?? "") === value.trim()) return;
    await app.editRegistry(file, "save", id, { [key]: value });
  }

  const onEnter = (e: KeyboardEvent) => {
    if (e.key === "Enter") (e.currentTarget as HTMLElement).blur();
    if (e.key === "Escape") {
      const el = e.currentTarget as HTMLInputElement;
      el.value = el.defaultValue;
      el.blur();
    }
  };

  // ------------------------------------------------------------ adding

  let newCue = $state("");
  let newName = $state("");
  let newPlace = $state("");

  async function addCharacter(cue: string, extra: Record<string, unknown> = {}) {
    if (!cue.trim()) return;
    if (await app.editRegistry("characters.yaml", "save", null, { cue: cue.trim().toUpperCase(), ...extra })) {
      newCue = "";
      newName = "";
    }
  }

  async function addPlace(name: string) {
    if (!name.trim()) return;
    if (await app.editRegistry("locations.yaml", "save", null, { name: name.trim() })) newPlace = "";
  }

  /** A heading place like "THE QUAY" listed as "The quay". */
  const sentenceCase = (s: string) => s.charAt(0) + s.slice(1).toLowerCase();

  // ------------------------------------------------------------ deleting

  let confirmDelete = $state<string | null>(null);
  async function remove(file: "characters.yaml" | "locations.yaml", id: string) {
    const key = `${file}:${id}`;
    if (confirmDelete !== key) {
      confirmDelete = key;
      setTimeout(() => (confirmDelete === key ? (confirmDelete = null) : null), 3000);
      return;
    }
    confirmDelete = null;
    await app.editRegistry(file, "delete", id);
  }

  // ------------------------------------------------------------ renaming

  let renaming = $state<{ kind: "character" | "place"; old: string; next: string; preview: RenameResult | null } | null>(null);

  function startRename(kind: "character" | "place", old: string) {
    renaming = { kind, old, next: old, preview: null };
  }

  async function preview() {
    if (!renaming) return;
    const r = renaming;
    if (r.kind === "character") r.next = r.next.toUpperCase();
    r.preview = await app.renameEverywhere(r.kind, r.old, r.next.trim(), false);
  }

  async function applyRename() {
    if (!renaming?.preview) return;
    const done = await app.renameEverywhere(renaming.kind, renaming.old, renaming.next.trim(), true);
    if (done) renaming = null;
  }

  function describe(r: RenameResult, kind: "character" | "place"): string {
    const parts: string[] = [];
    const n = (x: number, one: string, many: string) => `${x} ${x === 1 ? one : many}`;
    if (r.cues) parts.push(n(r.cues, "cue", "cues"));
    if (r.songs) parts.push(n(r.songs, "song", "songs"));
    if (r.mentions) parts.push(n(r.mentions, "mention in the action", "mentions in the action"));
    if (r.headings) parts.push(n(r.headings, "scene heading", "scene headings"));
    if (r.silent) parts.push(n(r.silent, "silent appearance", "silent appearances"));
    const where = r.scenes ? ` in ${n(r.scenes, "scene", "scenes")}` : "";
    const lists = r.files.length ? `${parts.length ? ", and " : ""}${r.files.join(" and ")}` : "";
    if (!parts.length && !lists) return kind === "place" ? "Only the name in the list changes." : "Nothing to change.";
    return `Changes ${parts.join(", ")}${where}${lists}.`;
  }

  const sceneList = (labels: string[]) => (labels.length > 6 ? `${labels.slice(0, 6).join(", ")} +${labels.length - 6}` : labels.join(", "));
</script>

<div class="cast">
  <div class="head">
    <h2>Cast &amp; places</h2>
    <p class="muted small">
      Who and where the script uses, and the lists in characters.yaml and locations.yaml. Edits save as you leave a field.
    </p>
  </div>
  {#if err}<p class="err">{err}</p>{/if}

  {#if renaming}
    {@const r = renaming}
    <div class="rename" role="dialog" aria-label="Rename {r.old}">
      <label>
        <span>Rename <strong>{r.old}</strong> to</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={r.next}
          autofocus
          data-testid="rename-to"
          class:caps={r.kind === "character"}
          oninput={() => (r.preview = null)}
          onkeydown={(e) => e.key === "Enter" && (r.preview ? applyRename() : preview())}
        />
      </label>
      {#if r.preview}
        <p data-testid="rename-preview">
          {describe(r.preview, r.kind)}
          {#if r.preview.merge}<br /><strong>{r.next.toUpperCase()}</strong> is already in the script: the two become one character.{/if}
          {#if r.kind === "character"}<br /><span class="muted">Dialogue isn't changed: people still say the old name.</span>{/if}
        </p>
      {/if}
      <div class="row">
        <button class="ghost" onclick={() => (renaming = null)}>Cancel</button>
        {#if r.preview}
          <button class="primary" onclick={applyRename} data-testid="rename-apply">Rename everywhere</button>
        {:else}
          <button class="primary" onclick={preview} disabled={!r.next.trim() || r.next.trim() === r.old} data-testid="rename-preview-btn">
            Preview
          </button>
        {/if}
      </div>
    </div>
  {/if}

  {#if cast}
    <!-- ------------------------------------------------------ characters -->
    <section aria-label="Characters">
      <h3>Characters <span class="muted">· {cast.characters.length}</span></h3>

      {#if cast.unlisted_characters.length}
        <div class="callout" data-testid="unlisted-characters">
          <p><strong>In the script but not in the list.</strong> Add them, or rename a typo onto the right name.</p>
          <ul>
            {#each cast.unlisted_characters as u (u.cue)}
              <li>
                <span class="cue">{u.cue}</span>
                <span class="muted small">{u.speeches} {u.speeches === 1 ? "speech" : "speeches"} · {sceneList(u.scenes)}</span>
                <span class="actions">
                  <button class="ghost" onclick={() => startRename("character", u.cue)}>Rename…</button>
                  <button onclick={() => addCharacter(u.cue)} data-testid="add-{u.cue}">Add to list</button>
                </span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <table class="list">
        <thead>
          <tr><th>Cue</th><th>Name</th><th title="What the action calls them after their introduction">Short name</th><th>Description</th><th>Actor (track)</th><th>Scenes</th><th class="n">Speeches</th><th></th></tr>
        </thead>
        <tbody>
          {#each cast.characters as c (c.entry.id)}
            <tr data-cue={c.cue}>
              <td class="cue">{c.cue}</td>
              <td>
                <input
                  type="text"
                  value={c.entry.name ?? ""}
                  aria-label="Name of {c.cue}"
                  onkeydown={onEnter}
                  onblur={(e) => saveField("characters.yaml", c.entry.id, "name", e.currentTarget.value, c.entry.name)}
                />
              </td>
              <td>
                <input
                  type="text"
                  class="short"
                  value={c.entry.short ?? ""}
                  placeholder={c.entry.name?.split(" ")[0] ?? "—"}
                  aria-label="Short name of {c.cue}"
                  title="What the action calls {c.cue} after their first appearance"
                  onkeydown={onEnter}
                  onblur={(e) => saveField("characters.yaml", c.entry.id, "short", e.currentTarget.value, c.entry.short)}
                />
              </td>
              <td>
                <input
                  type="text"
                  value={c.entry.description ?? ""}
                  placeholder="—"
                  aria-label="Description of {c.cue}"
                  onkeydown={onEnter}
                  onblur={(e) => saveField("characters.yaml", c.entry.id, "description", e.currentTarget.value, c.entry.description)}
                />
              </td>
              <td>
                <input
                  type="text"
                  value={c.entry.track ?? ""}
                  placeholder="none"
                  list="tracks"
                  class="track"
                  aria-label="Track of {c.cue}"
                  onkeydown={onEnter}
                  onblur={(e) => saveField("characters.yaml", c.entry.id, "track", e.currentTarget.value, c.entry.track)}
                />
              </td>
              <td class="small muted" title={c.scenes.join(", ")}>{c.scenes.length ? sceneList(c.scenes) : "not in the script"}</td>
              <td class="n">{c.speeches}{c.silent ? ` +${c.silent}•` : ""}</td>
              <td class="row-actions">
                <button class="ghost" onclick={() => startRename("character", c.cue)} aria-label="Rename {c.cue}">Rename…</button>
                <button
                  class="ghost del"
                  class:confirm={confirmDelete === `characters.yaml:${c.entry.id}`}
                  title="Remove from the list (the script is not changed)"
                  aria-label="Remove {c.cue} from the list"
                  onclick={() => remove("characters.yaml", c.entry.id)}
                >{confirmDelete === `characters.yaml:${c.entry.id}` ? "Remove?" : "🗑"}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <datalist id="tracks">{#each trackChoices as t}<option value={t}></option>{/each}</datalist>

      <form class="add" onsubmit={(e) => (e.preventDefault(), addCharacter(newCue, newName.trim() ? { name: newName.trim() } : {}))}>
        <input type="text" bind:value={newCue} placeholder="CUE" class="caps" aria-label="New character's cue" data-testid="new-cue" />
        <input type="text" bind:value={newName} placeholder="Name (optional)" aria-label="New character's name" />
        <button disabled={!newCue.trim()}>Add character</button>
      </form>
      <p class="muted small">
        The cue is the name as it appears above the character's lines{screen
          ? "; in the action it introduces them, in capitals, and the short name is used after that"
          : ""}. The track groups roles one actor plays, for the
        doubling check{screen ? "" : " in Production"}.
      </p>
    </section>

    <!-- ------------------------------------------------------ places -->
    <section aria-label="Places">
      <h3>Places <span class="muted">· {cast.locations.length}</span></h3>

      {#if cast.unlisted_places.length}
        <div class="callout" data-testid="unlisted-places">
          <p>
            <strong>{screen ? "In scene headings but not in the list." : "Used by scenes but not in the list."}</strong>
          </p>
          <ul>
            {#each cast.unlisted_places as u (u.kind + u.place)}
              <li>
                <span class="cue">{u.place}</span>
                <span class="muted small">
                  {u.kind === "heading" ? `${u.headings} ${u.headings === 1 ? "heading" : "headings"} · ` : ""}{sceneList(u.scenes)}
                </span>
                <span class="actions">
                  {#if u.kind === "heading"}<button class="ghost" onclick={() => startRename("place", u.place)}>Rename…</button>{/if}
                  <button
                    onclick={() => (u.kind === "heading" ? addPlace(sentenceCase(u.place)) : app.editRegistry("locations.yaml", "save", null, { id: u.place, name: u.place }))}
                    data-testid="add-place-{u.place}">Add to list</button
                  >
                </span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <table class="list">
        <thead>
          <tr><th>Name</th>{#if screen}<th>In headings</th>{/if}<th>Description</th><th>Scenes</th><th></th></tr>
        </thead>
        <tbody>
          {#each cast.locations as l (l.id)}
            <tr data-place={l.id}>
              <td>
                {#if screen}
                  <!-- In film and TV the name is what headings say: change it with Rename. -->
                  {l.entry.name ?? l.id}
                {:else}
                  <input
                    type="text"
                    value={l.entry.name ?? ""}
                    aria-label="Name of place {l.id}"
                    onkeydown={onEnter}
                    onblur={(e) => saveField("locations.yaml", l.id, "name", e.currentTarget.value, l.entry.name)}
                  />
                {/if}
              </td>
              {#if screen}<td class="cue small">{l.heading}{l.headings ? ` ×${l.headings}` : ""}</td>{/if}
              <td>
                <input
                  type="text"
                  value={l.entry.description ?? ""}
                  placeholder="—"
                  aria-label="Description of place {l.id}"
                  onkeydown={onEnter}
                  onblur={(e) => saveField("locations.yaml", l.id, "description", e.currentTarget.value, l.entry.description)}
                />
              </td>
              <td class="small muted" title={l.scenes.join(", ")}>{l.scenes.length ? sceneList(l.scenes) : "not used yet"}</td>
              <td class="row-actions">
                {#if screen}<button class="ghost" onclick={() => startRename("place", l.heading)} aria-label="Rename place {l.entry.name ?? l.id}">Rename…</button>{/if}
                <button
                  class="ghost del"
                  class:confirm={confirmDelete === `locations.yaml:${l.id}`}
                  title="Remove from the list (the script is not changed)"
                  aria-label="Remove place {l.entry.name ?? l.id} from the list"
                  onclick={() => remove("locations.yaml", l.id)}
                >{confirmDelete === `locations.yaml:${l.id}` ? "Remove?" : "🗑"}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <form class="add" onsubmit={(e) => (e.preventDefault(), addPlace(newPlace))}>
        <input type="text" bind:value={newPlace} placeholder={screen ? "e.g. Harbour office" : "e.g. The quay at Saltmarsh"} aria-label="New place" data-testid="new-place" />
        <button disabled={!newPlace.trim()}>Add place</button>
      </form>
      {#if screen}
        <p class="muted small">A place matches scene headings that use its name in capitals; Rename changes both.</p>
      {/if}
    </section>
  {:else if !err}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .cast {
    height: 100%;
    overflow: auto;
    padding: 20px 28px 60px;
    max-width: 1100px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
  }
  h3 {
    font-size: 15px;
    margin: 28px 0 8px;
  }
  .small {
    font-size: 12.5px;
  }
  .head p {
    margin: 4px 0 0;
  }
  .err {
    color: var(--bad);
  }
  .callout {
    border: 1px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 8%, transparent);
    border-radius: var(--radius);
    padding: 8px 12px;
    margin-bottom: 12px;
    font-size: 13px;
  }
  .callout p {
    margin: 0 0 6px;
  }
  .callout ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .callout li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 3px 0;
  }
  .callout .actions {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }
  .cue {
    font-family: var(--script-font);
    font-weight: 700;
    letter-spacing: 0.03em;
    white-space: nowrap;
  }
  table.list {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  .list th,
  .list td {
    padding: 3px 6px;
    border-bottom: 1px solid var(--line);
    text-align: left;
    vertical-align: middle;
  }
  .list thead th {
    font-size: 11.5px;
    color: var(--ink-3);
    font-weight: 600;
  }
  .list input {
    width: 100%;
    border: 1px solid transparent;
    background: transparent;
    padding: 3px 5px;
    font: inherit;
  }
  .list input:hover {
    border-color: var(--line);
  }
  .list input:focus {
    border-color: var(--accent);
    background: var(--paper);
  }
  .list input.track {
    max-width: 9em;
  }
  .list input.short {
    max-width: 8em;
  }
  .n {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .row-actions {
    white-space: nowrap;
    text-align: right;
  }
  .del.confirm {
    color: var(--bad);
    font-weight: 600;
  }
  .add {
    display: flex;
    gap: 8px;
    margin: 10px 0 4px;
    max-width: 560px;
  }
  .add input {
    flex: 1;
  }
  .caps {
    text-transform: uppercase;
  }
  .rename {
    position: sticky;
    top: 0;
    z-index: 5;
    margin: 12px 0;
    padding: 12px 14px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: var(--paper);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.12);
    font-size: 13px;
    max-width: 640px;
  }
  .rename label {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .rename input {
    flex: 1;
  }
  .rename p {
    margin: 10px 0 0;
  }
  .rename .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
  }
</style>
