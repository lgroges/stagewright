<!-- Beats: the story's plan, from a template or from scratch, each beat linked to
     the scenes that carry it out, with where it lands against where it's aimed.
     A TV series has a sheet for the series and one per episode. -->
<script lang="ts">
  import type { Beat } from "../lib/api";
  import { Queue } from "../lib/queue.svelte";
  import { app } from "../lib/state.svelte";

  const notes = app.notes;
  $effect(() => {
    void app.project;
    void notes.load();
  });

  const series = $derived(app.project?.series ?? false);
  /** Which sheet: null = the play's, film's or series' own; a number = an episode's. */
  let episode = $state<number | null>(null);
  const sheet = $derived(notes.sheet(episode));
  const templates = $derived(notes.index?.templates ?? []);
  const scenes = $derived(app.allScenes().filter((s) => episode === null || s.episode === episode));
  const sceneName = (id: string) => {
    const s = app.allScenes().find((x) => x.id === id);
    return s ? `${s.label} ${s.title}` : `${id} (missing)`;
  };

  // A copy to edit; refreshed from the saved sheet (which also says where beats land).
  let beats = $state<Beat[]>([]);
  $effect(() => {
    beats = (structuredClone($state.snapshot(sheet?.beats ?? [])) as Beat[]).map((b) => ({ ...b, scenes: b.scenes ?? [], episodes: b.episodes ?? [] }));
  });

  // Saves go one after another, each with the beats as they are by then.
  const saves = new Queue();
  function save() {
    void saves.run(() => notes.saveSheet(episode, sheet?.template ?? null, $state.snapshot(beats) as Beat[]));
  }

  function edit(i: number, patch: Partial<Beat>) {
    beats[i] = { ...beats[i], ...patch };
    save();
  }
  function move(i: number, by: number) {
    const j = i + by;
    if (j < 0 || j >= beats.length) return;
    const next = [...beats];
    [next[i], next[j]] = [next[j], next[i]];
    beats = next;
    save();
  }
  function remove(i: number) {
    beats = beats.filter((_, k) => k !== i);
    save();
  }
  function addBeat() {
    const taken = new Set(notes.index?.records.map((r) => r.id) ?? []);
    const prefix = episode === null ? "beat" : `e${episode}-beat`;
    let n = beats.length + 1;
    while (taken.has(`${prefix}-${n}`) || beats.some((b) => b.id === `${prefix}-${n}`)) n++;
    beats = [...beats, { id: `${prefix}-${n}`, title: "New beat", scenes: [], episodes: [] }];
    save();
  }

  const pct = (x: number | null | undefined) => (x == null ? "" : `${Math.round(x * 100)}%`);
  /** How far a beat lands from its aim, in points of the length; > 10 is notable. */
  const off = (b: Beat) => (b.lands != null && b.target != null ? Math.round((b.lands - b.target) * 100) : null);

  let chosenTemplate = $state("");
  async function start(template: string) {
    if (sheet?.beats.length && !confirm("Replace this beat sheet with the template? Its beats and their scenes will be lost.")) return;
    await notes.startSheet(episode, template);
    chosenTemplate = "";
  }
</script>

<div class="beats" data-testid="beats-view">
  <header>
    <h2>Beats</h2>
    {#if series}
      <select bind:value={episode} aria-label="Which beat sheet" data-testid="beat-sheet">
        <option value={null}>The series</option>
        {#each app.project?.episodes ?? [] as e (e.number)}
          <option value={e.number}>Episode {e.number}{e.title ? `: ${e.title}` : ""}</option>
        {/each}
      </select>
    {/if}
    {#if sheet?.template}<span class="muted small">from {templates.find((t) => t.id === sheet.template)?.name ?? sheet.template}</span>{/if}
    {#if saves.pending > 0}<span class="muted small">Saving…</span>{/if}
    {#if sheet}
      <select class="replace" bind:value={chosenTemplate} onchange={() => chosenTemplate && start(chosenTemplate)} aria-label="Start again from a template">
        <option value="">Start again from…</option>
        {#each templates as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
      </select>
    {/if}
  </header>
  {#if notes.error}<p class="bad small" role="alert">{notes.error}</p>{/if}

  {#if !sheet}
    <div class="start" data-testid="beat-start">
      <p>
        A beat sheet is the plan of the {episode === null ? (series ? "series" : "story") : "episode"}: its turning points,
        each linked to the scenes that carry it out, with where it lands against where it's aimed. Start from a template
        (nothing is enforced; change anything) or from nothing.
      </p>
      <div class="choices">
        {#each templates as t (t.id)}
          <button onclick={() => start(t.id)} data-testid={`template-${t.id}`}>{t.name}</button>
        {/each}
        <button class="ghost" onclick={() => start("")}>Blank</button>
      </div>
    </div>
  {:else}
    <!-- Where each beat is aimed (open) and lands (filled), along the length. -->
    <div class="strip" aria-hidden="true">
      {#each beats as b, i (b.id)}
        {#if b.target != null}<span class="aim" style:left={`${b.target * 100}%`} title={`${b.title}: aimed at ${pct(b.target)}`}></span>{/if}
        {#if b.lands != null}
          <span class="land" class:far={Math.abs(off(b) ?? 0) > 10} style:left={`${b.lands * 100}%`} title={`${b.title}: lands at ${pct(b.lands)}`}>{i + 1}</span>
        {/if}
      {/each}
    </div>

    <ol class="list">
      {#each beats as b, i (b.id)}
        <li class="beat" data-testid="beat">
          <div class="num">{i + 1}</div>
          <div class="main">
            <input type="text" class="title" value={b.title} onchange={(e) => edit(i, { title: e.currentTarget.value.trim() || b.title })} aria-label="Beat" />
            <textarea
              rows="2"
              placeholder="What happens"
              value={b.summary ?? ""}
              onchange={(e) => edit(i, { summary: e.currentTarget.value.trim() || null })}
            ></textarea>
            <textarea
              class="purpose"
              rows="1"
              placeholder="What it's for"
              value={b.purpose ?? ""}
              onchange={(e) => edit(i, { purpose: e.currentTarget.value.trim() || null })}
            ></textarea>
            <div class="scenes">
              {#each b.scenes as id (id)}
                <span class="chip">
                  <button class="ghost link" onclick={() => notes.show({ id, kind: "scene" })}>{sceneName(id)}</button>
                  <button class="ghost x" aria-label="Unlink scene" onclick={() => edit(i, { scenes: b.scenes.filter((s) => s !== id) })}>✕</button>
                </span>
              {/each}
              <select
                aria-label="Link a scene"
                value=""
                onchange={(e) => e.currentTarget.value && edit(i, { scenes: [...b.scenes, e.currentTarget.value] })}
                data-testid="beat-add-scene"
              >
                <option value="">+ scene…</option>
                {#each scenes.filter((s) => !b.scenes.includes(s.id)) as s (s.id)}<option value={s.id}>{s.label} {s.title}</option>{/each}
              </select>
              {#if series && episode === null}
                <input
                  type="text"
                  class="eps"
                  placeholder="episodes, e.g. 1, 2"
                  value={b.episodes.join(", ")}
                  onchange={(e) =>
                    edit(i, {
                      episodes: e.currentTarget.value
                        .split(",")
                        .map((x) => parseInt(x, 10))
                        .filter((x) => x > 0),
                    })}
                />
              {/if}
              {#if !b.scenes.length && !(series && episode === null)}
                <button class="ghost small" onclick={() => notes.makeScene(episode, b)} data-testid="beat-make-scene">Make a scene for it</button>
              {/if}
            </div>
          </div>
          <div class="where">
            <label class="small muted">
              aim
              <input
                type="number"
                min="0"
                max="100"
                value={b.target == null ? "" : Math.round(b.target * 100)}
                onchange={(e) => edit(i, { target: e.currentTarget.value === "" ? null : Math.min(100, Math.max(0, Number(e.currentTarget.value))) / 100 })}
                aria-label="Aim, as a percentage of the length"
              />%
            </label>
            {#if b.lands != null}
              <div class="lands" class:far={Math.abs(off(b) ?? 0) > 10} data-testid="beat-lands">
                lands {pct(b.lands)}{b.lands_at ? `, ${b.lands_at}` : ""}
                {#if off(b) != null && off(b) !== 0}<span class="small">({off(b)! > 0 ? "+" : ""}{off(b)})</span>{/if}
              </div>
            {:else}
              <div class="muted small">no scene yet</div>
            {/if}
            <div class="moves">
              <button class="ghost small" aria-label="Move up" disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
              <button class="ghost small" aria-label="Move down" disabled={i === beats.length - 1} onclick={() => move(i, 1)}>↓</button>
              <button class="ghost small" aria-label="Delete beat" onclick={() => remove(i)}>✕</button>
            </div>
          </div>
        </li>
      {/each}
    </ol>
    <button class="ghost" onclick={addBeat} data-testid="add-beat">+ Add a beat</button>
  {/if}
</div>

<style>
  .beats {
    overflow: auto;
    height: 100%;
    padding: 20px 28px 40px;
    max-width: 1000px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }
  header h2 {
    margin: 0;
    font-size: 22px;
  }
  .replace {
    margin-left: auto;
    width: auto;
  }
  header select {
    width: auto;
  }
  header .muted {
    white-space: nowrap;
  }
  .start {
    max-width: 640px;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .strip {
    position: relative;
    height: 26px;
    margin: 4px 14px 18px;
    border-bottom: 2px solid var(--line);
  }
  .strip span {
    position: absolute;
    bottom: -7px;
    transform: translateX(-50%);
  }
  .aim {
    width: 10px;
    height: 10px;
    border: 2px solid var(--ink-3);
    border-radius: 50%;
    background: var(--bg);
  }
  .land {
    min-width: 18px;
    height: 18px;
    bottom: -9px !important;
    border-radius: 9px;
    background: var(--accent);
    color: #fff;
    font-size: 10.5px;
    font-weight: 600;
    text-align: center;
    line-height: 18px;
  }
  .land.far {
    background: var(--warn);
  }
  .list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .beat {
    display: grid;
    grid-template-columns: 28px 1fr 170px;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--paper);
  }
  .num {
    font-weight: 600;
    color: var(--ink-3);
    padding-top: 4px;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .title {
    font-weight: 600;
    font-size: 15px;
  }
  textarea {
    resize: vertical;
    font: inherit;
    font-size: 13px;
  }
  .purpose {
    color: var(--ink-2);
    font-style: italic;
  }
  .scenes select {
    width: auto;
    max-width: 220px;
  }
  .scenes {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
    font-size: 12.5px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 0 2px 0 8px;
    background: var(--panel);
  }
  .chip .x {
    font-size: 10px;
    padding: 0 5px;
  }
  .eps {
    width: 150px;
  }
  .where {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
  }
  .where input {
    width: 54px;
  }
  .lands.far {
    color: var(--warn);
  }
  .moves {
    margin-top: auto;
    display: flex;
    gap: 2px;
  }
  .bad {
    color: var(--bad);
  }
</style>
