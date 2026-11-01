<script lang="ts">
  import { app } from "../lib/state.svelte";
  import type { SceneMeta } from "../lib/api";

  // Drafts for free-text fields: reset only when a different scene opens, so a
  // background save never wipes what is being typed.
  let draftId = $state<string | null>(null);
  let title = $state("");
  let synopsis = $state("");
  let notes = $state("");
  let tags = $state("");
  // bound to a number input: Svelte gives a number (or null when empty)
  let duration = $state<string | number | null>("");

  $effect(() => {
    const s = app.scene;
    if (!s || s.id === draftId) return;
    draftId = s.id;
    title = s.meta.title ?? "";
    synopsis = s.meta.synopsis ?? "";
    notes = s.meta.notes ?? "";
    tags = s.meta.tags.join(", ");
    duration = s.meta.duration?.toString() ?? "";
  });

  const meta = $derived(app.scene?.meta);
  const speakers = $derived(app.scene?.summary.speakers ?? []);
  const silentChoices = $derived(
    (app.project?.characters ?? []).map((c) => c.cue).filter((c) => !speakers.includes(c)),
  );

  async function save(patch: Partial<SceneMeta>) {
    await app.updateMeta(patch);
  }

  function commitTitle() {
    const t = title.trim();
    if (t && t !== meta?.title) save({ title: t });
    else title = meta?.title ?? "";
  }

  function commitText(key: "synopsis" | "notes", value: string) {
    const v = value.trim() === "" ? null : value;
    if (v !== (meta?.[key] ?? null)) save({ [key]: v });
  }

  function commitTags() {
    const list = tags.split(",").map((t) => t.trim()).filter(Boolean);
    if (list.join(",") !== meta?.tags.join(",")) save({ tags: list });
  }

  function commitDuration() {
    const raw = String(duration ?? "").trim();
    const v = raw === "" ? null : Number(raw);
    if (v !== null && !(v > 0 && v < 600)) {
      duration = meta?.duration?.toString() ?? "";
      return;
    }
    if (v !== (meta?.duration ?? null)) save({ duration: v });
  }

  function toggleSilent(cue: string) {
    const cur = meta?.onstage_silent ?? [];
    save({ onstage_silent: cur.includes(cue) ? cur.filter((c) => c !== cue) : [...cur, cue] });
  }

  function setRating(field: string, value: number | null) {
    const r = { ...(meta?.ratings ?? {}) };
    if (value === null) delete r[field];
    else r[field] = value;
    save({ ratings: r });
  }

  const enterBlurs = (e: KeyboardEvent) => {
    if (e.key === "Enter") (e.currentTarget as HTMLElement).blur();
  };
</script>

{#if app.scene && meta}
  <div class="panel">
    <label class="field">
      <span>Title</span>
      <input type="text" bind:value={title} onblur={commitTitle} onkeydown={enterBlurs} data-testid="meta-title" />
    </label>

    <div class="two">
      <label class="field">
        <span>Status</span>
        <select value={meta.status ?? ""} onchange={(e) => save({ status: (e.currentTarget as HTMLSelectElement).value })}>
          {#if !meta.status}<option value="" disabled>—</option>{/if}
          {#each app.project?.statuses ?? [] as s}<option value={s}>{s.replace("-", " ")}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>Runs (min)</span>
        <input
          type="number"
          min="0.5"
          max="599"
          step="0.5"
          bind:value={duration}
          placeholder={app.scene.summary.manual_duration ? "" : `≈ ${app.scene.summary.minutes.toFixed(1)}`}
          onblur={commitDuration}
          onkeydown={enterBlurs}
          title="Leave empty to use the estimate from the text"
          data-testid="meta-duration"
        />
      </label>
    </div>

    <label class="field">
      <span>Location</span>
      <select
        value={meta.location ?? ""}
        onchange={(e) => {
          const v = (e.currentTarget as HTMLSelectElement).value;
          save({ location: v === "" ? null : v });
        }}
      >
        <option value="">—</option>
        {#each app.project?.locations ?? [] as l}<option value={l.id}>{l.name ?? l.id}</option>{/each}
        {#if meta.location && !(app.project?.locations ?? []).some((l) => l.id === meta.location)}
          <option value={meta.location}>{meta.location} (not in locations.yaml)</option>
        {/if}
      </select>
    </label>

    <label class="field">
      <span>Synopsis</span>
      <textarea rows="3" bind:value={synopsis} onblur={() => commitText("synopsis", synopsis)}></textarea>
    </label>

    <div class="field">
      <span class="label">In this scene</span>
      <div class="chips">
        {#each speakers as c}<span class="chip speaking" title="Speaks in this scene">{c}</span>{/each}
        {#each meta.onstage_silent as c}
          <button class="chip silent on" onclick={() => toggleSilent(c)} title="On stage, silent. Click to remove.">{c} ✕</button>
        {/each}
      </div>
      {#if silentChoices.filter((c) => !meta.onstage_silent.includes(c)).length}
        <select
          class="add-silent"
          value=""
          onchange={(e) => {
            const v = (e.currentTarget as HTMLSelectElement).value;
            if (v) toggleSilent(v);
            (e.currentTarget as HTMLSelectElement).value = "";
          }}
        >
          <option value="">{app.project?.format === "stage" || !app.project ? "+ on stage, silent…" : "+ in the scene, silent…"}</option>
          {#each silentChoices.filter((c) => !meta.onstage_silent.includes(c)) as c}<option value={c}>{c}</option>{/each}
        </select>
      {/if}
    </div>

    {#if (app.project?.rating_fields ?? []).length}
      <div class="field">
        <span class="label">Ratings</span>
        {#each app.project?.rating_fields ?? [] as f}
          <div class="rating">
            <span class="rname">{f}</span>
            <input
              type="range"
              min="1"
              max="9"
              value={meta.ratings[f] ?? 5}
              class:unset={meta.ratings[f] === undefined}
              aria-label="{f} rating"
              onchange={(e) => setRating(f, Number((e.currentTarget as HTMLInputElement).value))}
            />
            <span class="rval">{meta.ratings[f] ?? "–"}</span>
            {#if meta.ratings[f] !== undefined}
              <button class="ghost tiny" onclick={() => setRating(f, null)} aria-label="Clear {f}">✕</button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}

    <label class="field">
      <span>Tags</span>
      <input type="text" bind:value={tags} onblur={commitTags} onkeydown={enterBlurs} placeholder="comma, separated" />
    </label>

    <label class="field">
      <span>Notes</span>
      <textarea rows="4" bind:value={notes} onblur={() => commitText("notes", notes)}></textarea>
    </label>

    <p class="muted small">ID {app.scene.id} · {app.scene.summary.file}</p>
  </div>
{/if}

<style>
  .panel {
    padding: 16px;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .label {
    display: block;
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ink-3);
    margin-bottom: 6px;
  }
  .field {
    margin-bottom: 14px;
  }
  textarea {
    resize: vertical;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 6px;
  }
  .chip {
    font: 600 11px var(--ui-font);
    letter-spacing: 0.03em;
    padding: 2px 7px;
    border-radius: 99px;
    border: 1px solid var(--line);
    background: var(--paper);
  }
  .chip.speaking {
    border-color: var(--accent);
    color: var(--accent);
  }
  .chip.silent {
    border-style: dashed;
  }
  .add-silent {
    font-size: 12px;
    padding: 3px 6px;
  }
  .rating {
    display: grid;
    grid-template-columns: 6.5em 1fr 1.4em 1.6em;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .rname {
    text-transform: capitalize;
  }
  .rval {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  input[type="range"] {
    accent-color: var(--accent);
  }
  input[type="range"].unset {
    opacity: 0.35;
  }
  .tiny {
    padding: 0 4px;
    font-size: 11px;
  }
  .small {
    font-size: 11.5px;
    word-break: break-all;
  }
</style>
