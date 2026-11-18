<!-- Beside the script: what the open scene is linked to (its beats, characters
     and place, with their notes) and research linked to any of them. The same
     set the AI's editorial read will get. -->
<script lang="ts">
  import { api, type ContextItem } from "../lib/api";
  import { app } from "../lib/state.svelte";

  let items = $state<ContextItem[]>([]);
  let open = $state<Record<string, boolean>>({});

  $effect(() => {
    const id = app.scene?.id;
    void app.notes.version;
    void app.project;
    if (!id) return;
    api
      .sceneContext(id)
      .then((r) => {
        if (app.scene?.id === id) items = r.items;
      })
      .catch(() => (items = []));
  });

  const groups = $derived(
    (
      [
        ["beat", "Beats"],
        ["character", "Characters"],
        ["location", "Place"],
        ["research", "Research"],
      ] as const
    )
      .map(([kind, title]) => ({ kind, title, items: items.filter((i) => i.kind === kind) }))
      .filter((g) => g.items.length),
  );

  function show(i: ContextItem) {
    if (i.kind === "beat") app.showView("beats");
    else app.notes.open(i.id);
  }
  const short = (t: string) => (t.length > 180 ? t.slice(0, 179).trimEnd() + "…" : t);
</script>

<section class="context" data-testid="scene-context">
  <h3>Around this scene</h3>
  {#if !groups.length}
    <p class="muted small">
      Nothing linked yet. Notes on its characters and place, research linked to them, and the beats it carries out show
      here.
    </p>
  {/if}
  {#each groups as g (g.kind)}
    <h4>{g.title}</h4>
    <ul>
      {#each g.items as i (i.id)}
        <li>
          <button class="ghost name" onclick={() => show(i)}>{i.name}</button>
          {#if i.via}<span class="muted small">via {i.via}</span>{/if}
          {#if i.kind === "beat" && (i.summary || i.purpose)}
            <p class="small">{i.summary ?? ""}{#if i.purpose}<span class="muted"> {i.purpose}</span>{/if}</p>
          {:else if i.text}
            <p class="small text" class:full={open[i.id]}>
              {open[i.id] ? i.text : short(i.text)}
              {#if i.text.length > 180}
                <button class="ghost more" onclick={() => (open[i.id] = !open[i.id])}>{open[i.id] ? "less" : "more"}</button>
              {/if}
            </p>
          {/if}
        </li>
      {/each}
    </ul>
  {/each}
</section>

<style>
  .context {
    margin-top: 18px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }
  h3 {
    margin: 0 0 4px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ink-3);
  }
  h4 {
    margin: 10px 0 2px;
    font-size: 11.5px;
    color: var(--ink-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    padding: 3px 0;
  }
  .name {
    padding: 0;
    font-weight: 600;
    font-size: 13px;
  }
  p {
    margin: 2px 0 0;
    white-space: pre-line;
  }
  .more {
    padding: 0 4px;
    font-size: 11px;
  }
</style>
