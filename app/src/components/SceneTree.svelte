<script lang="ts">
  import { app } from "../lib/state.svelte";
  import type { Act, SceneSummary } from "../lib/api";

  type Drop = { key: string; index: number };
  let dragId = $state<string | null>(null);
  let drop = $state<Drop | null>(null);
  let confirmTrash = $state<string | null>(null);
  /** An act or episode waiting for "Delete" to be confirmed. */
  let confirmDelete = $state<{ episode: number | null; act: number | null } | null>(null);

  const keyOf = (a: Act) => `${a.episode ?? ""}:${a.number}`;
  const screen = $derived(app.project ? app.project.format !== "stage" : false);
  /** In a series the episode is shown by the group, so drop the "E1 " prefix. */
  const label = (s: SceneSummary) => s.label.replace(/^E\d+ /, "");
  const mins = (m: number) => (m < 1 ? "<1" : Math.round(m).toString());
  const actMinutes = (act: Act) => act.scenes.reduce((t, s) => t + s.minutes, 0);
  const actPages = (act: Act) => {
    const e = act.scenes.reduce((t, s) => t + s.eighths, 0);
    const w = Math.floor(e / 8), p = e % 8;
    return w && p ? `${w} ${p}/8` : w ? `${w}` : `${p}/8`;
  };

  /** Acts grouped by episode (one group with episode null outside a series). */
  const groups = $derived.by(() => {
    const acts = app.project?.acts ?? [];
    if (!app.project?.series) return [{ episode: null as number | null, title: "", acts }];
    return (app.project.episodes ?? []).map((e) => ({
      episode: e.number as number | null,
      title: e.title,
      acts: acts.filter((a) => a.episode === e.number),
    }));
  });
  const allActs = $derived(groups.flatMap((g) => g.acts));

  // ------------------------------------------------------------ folding
  const epKey = (e: number | null) => `e:${e}`;
  const actKey = (a: Act) => `a:${a.episode ?? ""}:${a.number}`;
  const isFolded = (k: string) => app.folded.has(k);
  const toggle = (k: string) => app.setFolded([k], !isFolded(k));
  const allKeys = $derived([...groups.filter((g) => g.episode !== null).map((g) => epKey(g.episode)), ...allActs.map(actKey)]);
  const anyFolded = $derived(allKeys.some((k) => isFolded(k)));

  // The open scene is always visible: unfold its act and episode.
  $effect(() => {
    const id = app.scene?.id;
    if (!id) return;
    const act = allActs.find((a) => a.scenes.some((s) => s.id === id));
    if (!act) return;
    const keys = [actKey(act), ...(act.episode != null ? [epKey(act.episode)] : [])].filter((k) => app.folded.has(k));
    if (keys.length) app.setFolded(keys, false);
  });

  function onDragStart(e: DragEvent, id: string) {
    dragId = id;
    e.dataTransfer?.setData("text/plain", id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function overScene(e: DragEvent, act: Act, index: number) {
    if (!dragId) return;
    e.preventDefault();
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    drop = { key: keyOf(act), index: e.clientY > r.top + r.height / 2 ? index + 1 : index };
  }

  function overAct(e: DragEvent, act: Act) {
    if (!dragId) return;
    e.preventDefault();
    if (!drop || drop.key !== keyOf(act)) drop = { key: keyOf(act), index: act.scenes.length };
  }

  async function onDrop(e: DragEvent) {
    e.preventDefault();
    const id = dragId;
    const target = drop;
    dragId = null;
    drop = null;
    if (!id || !target) return;
    const act = allActs.find((a) => keyOf(a) === target.key);
    if (!act) return;
    // Index is in the list *with* the dragged scene; adjust when moving down within an act.
    const from = act.scenes.findIndex((s) => s.id === id);
    const index = from >= 0 && from < target.index ? target.index - 1 : target.index;
    await app.moveScene(id, act.number, index, act.episode ?? null);
  }

  function nudge(act: Act, i: number, delta: number) {
    const s = act.scenes[i];
    const ai = allActs.indexOf(act);
    const j = i + delta;
    if (j >= 0 && j < act.scenes.length) return app.moveScene(s.id, act.number, j, act.episode ?? null);
    // Off the end of an act: move into the neighbouring act (or episode).
    const other = allActs[ai + delta];
    if (other) return app.moveScene(s.id, other.number, delta < 0 ? other.scenes.length : 0, other.episode ?? null);
  }

  const sceneCount = (episode: number | null, act: number | null) =>
    allActs.filter((a) => (a.episode ?? null) === episode && (act === null || a.number === act)).reduce((t, a) => t + a.scenes.length, 0);
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const asking = (episode: number | null, act: number | null) =>
    confirmDelete !== null && confirmDelete.episode === episode && confirmDelete.act === act;

  async function confirmed() {
    const c = confirmDelete;
    confirmDelete = null;
    if (!c) return;
    if (c.act === null && c.episode !== null) await app.deleteEpisode(c.episode);
    else if (c.act !== null) await app.deleteAct(c.act, c.episode);
  }

  async function trash(id: string) {
    if (confirmTrash !== id) {
      confirmTrash = id;
      setTimeout(() => (confirmTrash === id ? (confirmTrash = null) : null), 3000);
      return;
    }
    confirmTrash = null;
    await app.trashScene(id);
  }
</script>

<div class="tree" role="tree" aria-label={app.project?.series ? "Episodes, acts and scenes" : "Acts and scenes"}>
  {#each groups as g (g.episode ?? "all")}
    {#if g.episode !== null}
      <div class="episode-head" data-episode={g.episode}>
        <button
          class="ghost fold"
          class:folded={isFolded(epKey(g.episode))}
          onclick={() => toggle(epKey(g.episode))}
          aria-expanded={!isFolded(epKey(g.episode))}
          aria-label="{isFolded(epKey(g.episode)) ? 'Show' : 'Hide'} episode {g.episode}"
        >▾</button>
        <span class="ep-num">{g.episode}</span>
        <span class="ep-title">{g.title}</span>
        <span class="tools">
          {#if groups.length > 1}
            <button
              class="ghost del"
              title="Delete this episode"
              aria-label="Delete episode {g.episode}"
              onclick={() => (confirmDelete = { episode: g.episode, act: null })}>🗑</button
            >
          {/if}
          <button class="ghost add" title="Add an act to this episode" onclick={() => app.createAct(g.episode)}>+ act</button>
        </span>
      </div>
      {#if asking(g.episode, null)}
        {@const n = sceneCount(g.episode, null)}
        <div class="confirm" role="alertdialog" aria-label="Confirm deleting episode {g.episode}">
          <p>
            Delete episode {g.episode}, “{g.title}”{n ? `, and move its ${plural(n, "scene")} to the trash` : ""}?
            {#if g.episode !== null && g.episode < groups.length}Later episodes move up one.{/if}
          </p>
          <div class="row">
            <button class="ghost" onclick={() => (confirmDelete = null)}>Cancel</button>
            <button class="danger" onclick={confirmed} data-testid="confirm-delete">Delete episode</button>
          </div>
        </div>
      {/if}
    {/if}
    {#if g.episode !== null && isFolded(epKey(g.episode))}
      <p class="folded-note muted small">
        {g.acts.length} {g.acts.length === 1 ? "act" : "acts"} · {g.acts.reduce((t, a) => t + a.scenes.length, 0)} scenes
      </p>
    {:else}
    {#each g.acts as act (keyOf(act))}
      <section
        class="act"
        class:in-episode={g.episode !== null}
        role="group"
        aria-label={g.episode !== null ? `Episode ${g.episode}, ${act.title}` : act.title}
        ondragover={(e) => overAct(e, act)}
        ondrop={onDrop}
      >
        <div class="act-head">
          <button
            class="ghost fold"
            class:folded={isFolded(actKey(act))}
            onclick={() => toggle(actKey(act))}
            aria-expanded={!isFolded(actKey(act))}
            aria-label="{isFolded(actKey(act)) ? 'Show' : 'Hide'} the scenes of {g.episode !== null ? `episode ${g.episode}, ` : ''}{act.title}"
          >▾</button>
          <span class="act-title">{act.title}</span>
          {#if isFolded(actKey(act))}<span class="muted small">· {act.scenes.length}</span>{/if}
          <span class="muted small">{screen ? `${actPages(act)} pp` : `${mins(actMinutes(act))} min`}</span>
          <span class="tools">
            {#if g.acts.length > 1}
              <button
                class="ghost del"
                title="Delete {act.title}"
                aria-label="Delete {g.episode !== null ? `episode ${g.episode}, ` : ""}{act.title}"
                onclick={() => (confirmDelete = { episode: act.episode ?? null, act: act.number })}>🗑</button
              >
            {/if}
            <button
              class="ghost add"
              title="Add a scene at the end of {act.title}"
              onclick={() => app.createScene(act.number, act.scenes.at(-1)?.id ?? null, act.episode ?? null)}>+</button
            >
          </span>
        </div>
        {#if asking(act.episode ?? null, act.number)}
          <div class="confirm" role="alertdialog" aria-label="Confirm deleting {act.title}">
            <p>
              Delete {act.title}{act.scenes.length ? ` and move its ${plural(act.scenes.length, "scene")} to the trash` : ""}?
              {#if act.number >= 1 && g.acts.some((a) => a.number > act.number)}Later acts move up one.{/if}
            </p>
            <div class="row">
              <button class="ghost" onclick={() => (confirmDelete = null)}>Cancel</button>
              <button class="danger" onclick={confirmed} data-testid="confirm-delete">Delete act</button>
            </div>
          </div>
        {/if}
        <ol class:hidden={isFolded(actKey(act))}>
          {#each act.scenes as s, i (s.id)}
            {#if drop && drop.key === keyOf(act) && drop.index === i}<li class="drop-line" aria-hidden="true"></li>{/if}
            <li
              class="scene"
              class:selected={app.scene?.id === s.id}
              class:dragging={dragId === s.id}
              draggable="true"
              role="treeitem"
              aria-selected={app.scene?.id === s.id}
              data-scene-id={s.id}
              ondragstart={(e) => onDragStart(e, s.id)}
              ondragend={() => ((dragId = null), (drop = null))}
              ondragover={(e) => overScene(e, act, i)}
            >
              <button class="open" onclick={() => app.openScene(s.id)} title={s.headings?.join(" / ") || s.title}>
                <span class="num">{label(s)}</span>
                <span class="name">{s.title}</span>
                <span
                  class="mins"
                  title={s.manual_duration
                    ? "Manual duration"
                    : screen
                      ? `${s.pages} pages ≈ ${mins(s.minutes)} min`
                      : "Estimated from the text"}
                >
                  {screen && !s.manual_duration ? s.pages : mins(s.minutes)}{s.manual_duration ? "*" : ""}
                </span>
                <span class="marks">
                  {#each app.collab?.peers.filter((p) => p.scene === s.id) ?? [] as p (p.clientId)}
                    <span class="peer" style="background:{p.color}" title="{p.name} is here"></span>
                  {/each}
                  <span class="dot status-{s.status ?? 'none'}" title={s.status ?? "no status"}></span>
                </span>
              </button>
              <span class="actions">
                <button class="ghost" title="Move up" aria-label="Move {s.title} up" onclick={() => nudge(act, i, -1)}>↑</button>
                <button class="ghost" title="Move down" aria-label="Move {s.title} down" onclick={() => nudge(act, i, 1)}>↓</button>
                <button
                  class="ghost"
                  title="New scene after this one"
                  aria-label="New scene after {s.title}"
                  onclick={() => app.createScene(act.number, s.id, act.episode ?? null)}>+</button
                >
                <button
                  class="ghost trash"
                  class:confirm={confirmTrash === s.id}
                  title="Move to the project's .trash folder"
                  aria-label="Move {s.title} to trash"
                  onclick={() => trash(s.id)}>{confirmTrash === s.id ? "Trash?" : "🗑"}</button
                >
              </span>
            </li>
          {/each}
          {#if drop && drop.key === keyOf(act) && drop.index === act.scenes.length}<li class="drop-line" aria-hidden="true"></li>{/if}
          {#if act.scenes.length === 0}
            <li class="empty muted small">No scenes yet</li>
          {/if}
        </ol>
      </section>
    {/each}
    {/if}
  {/each}
  <div class="foot">
    {#if allKeys.length > 1}
      <button class="ghost small fold-all" onclick={() => app.setFolded(allKeys, !anyFolded)}>
        {anyFolded ? "Unfold all" : "Fold all"}
      </button>
    {/if}
    {#if app.project?.series}
      <button class="ghost" onclick={() => app.createEpisode()}>+ Add episode</button>
    {:else}
      <button class="ghost" onclick={() => app.createAct()}>+ Add act</button>
    {/if}
  </div>
</div>

<style>
  .tree {
    padding: 8px 0 24px;
    user-select: none;
  }
  .act {
    margin-bottom: 10px;
  }
  .act.in-episode .act-head {
    padding-left: 22px;
  }
  .act.in-episode .scene .open {
    padding-left: 22px;
  }
  .episode-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 10px 8px 2px 12px;
    border-top: 1px solid var(--line);
    margin-top: 4px;
  }
  .episode-head:first-child {
    border-top: 0;
    margin-top: 0;
  }
  .ep-num {
    font: 700 11px var(--ui-font);
    color: #fff;
    background: var(--accent);
    border-radius: 4px;
    padding: 0 5px;
  }
  .ep-title {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fold {
    padding: 0 2px;
    width: 18px;
    font-size: 11px;
    color: var(--ink-3);
    transition: transform 0.12s;
    margin-left: -6px;
  }
  .fold.folded {
    transform: rotate(-90deg);
  }
  ol.hidden {
    display: none;
  }
  .folded-note {
    margin: 0 0 6px 36px;
  }
  .fold-all {
    font-size: 12px;
  }
  .act-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 6px 8px 4px 14px;
  }
  .act-title {
    font-weight: 700;
    font-size: 12px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .tools {
    margin-left: auto;
    display: flex;
    align-items: baseline;
  }
  .add {
    padding: 0 8px;
    font-size: 16px;
    line-height: 1.2;
  }
  .del {
    padding: 0 6px;
    font-size: 12px;
    opacity: 0;
  }
  .act-head:hover .del,
  .act-head:focus-within .del,
  .episode-head:hover .del,
  .episode-head:focus-within .del {
    opacity: 0.7;
  }
  .del:hover,
  .del:focus-visible {
    opacity: 1 !important;
  }
  .confirm {
    margin: 2px 8px 8px 12px;
    padding: 8px 10px;
    border: 1px solid var(--bad);
    border-radius: var(--radius);
    background: var(--paper);
    font-size: 12.5px;
  }
  .confirm p {
    margin: 0 0 6px;
  }
  .confirm .row {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
  .danger {
    background: var(--bad);
    border-color: var(--bad);
    color: #fff;
  }
  .small {
    font-size: 12px;
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .scene {
    position: relative;
    display: flex;
    align-items: center;
  }
  .scene .open {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: 2.6em 1fr auto auto;
    gap: 6px;
    align-items: center;
    text-align: left;
    border: 0;
    border-radius: 0;
    background: transparent;
    padding: 6px 10px 6px 12px;
    border-left: 3px solid transparent;
  }
  .scene:hover .open {
    background: rgba(0, 0, 0, 0.035);
  }
  .scene.selected .open {
    background: var(--paper);
    border-left-color: var(--accent);
  }
  .scene.dragging {
    opacity: 0.4;
  }
  .num {
    font-variant-numeric: tabular-nums;
    color: var(--ink-3);
    font-size: 12px;
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mins {
    font-size: 12px;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }
  .marks {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .peer {
    width: 5px;
    height: 14px;
    border-radius: 3px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line);
  }
  .status-outline {
    background: transparent;
    border: 1.5px solid var(--ink-3);
  }
  .status-draft {
    background: var(--warn);
  }
  .status-first-edit,
  .status-second-edit {
    background: var(--song);
  }
  .status-done {
    background: var(--ok);
  }
  .actions {
    position: absolute;
    right: 4px;
    display: none;
    gap: 0;
    background: var(--panel);
    border-radius: var(--radius);
  }
  .scene:hover .actions,
  .scene:focus-within .actions {
    display: flex;
  }
  .scene.selected .actions {
    background: var(--paper);
  }
  .actions button {
    padding: 2px 6px;
    font-size: 12px;
    color: var(--ink-2);
  }
  .trash.confirm {
    color: #fff;
    background: var(--bad);
  }
  .drop-line {
    height: 2px;
    background: var(--accent);
    margin: 0 10px;
  }
  .empty {
    padding: 4px 14px 4px 26px;
  }
  .foot {
    padding: 6px 8px;
  }
</style>
