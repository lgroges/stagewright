<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ProjectCheck, type Stats, type StatsScene } from "../lib/api";
  import { app } from "../lib/state.svelte";

  let stats = $state<Stats | null>(null);
  let err = $state<string | null>(null);
  /** Series: which episode to show (null = the whole series). */
  let episode = $state<number | null>(null);

  async function load() {
    await app.flush();
    try {
      stats = await api.stats();
      err = null;
      if (stats.series && episode === null && app.scene?.summary.episode) episode = app.scene.summary.episode;
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    }
  }
  onMount(load);

  // ------------------------------------------------------------ script check
  let check = $state<ProjectCheck | null>(null);
  let showSettings = $state(false);
  async function loadCheck() {
    await app.flush();
    check = await api.checkProject().catch(() => null);
  }
  $effect(() => {
    void app.checksVersion;
    void app.project;
    void loadCheck();
  });
  const visibleIssues = $derived(
    (check?.issues ?? []).filter((i) => episode === null || stats?.scenes.find((s) => s.id === i.scene)?.episode === episode),
  );
  /** Issues grouped by scene, in running order. */
  const byScene = $derived.by(() => {
    const groups: { scene: string; label: string; title: string; issues: typeof visibleIssues }[] = [];
    for (const i of visibleIssues) {
      const last = groups.at(-1);
      if (last && last.scene === i.scene) last.issues.push(i);
      else groups.push({ scene: i.scene, label: i.label, title: i.title, issues: [i] });
    }
    return groups;
  });
  const counted = $derived({
    warnings: visibleIssues.filter((i) => i.severity === "warning").length,
    infos: visibleIssues.filter((i) => i.severity === "info").length,
  });

  const screen = $derived(stats ? stats.format !== "stage" : false);
  const scenes = $derived((stats?.scenes ?? []).filter((s) => episode === null || s.episode === episode));
  const acts = $derived((stats?.acts ?? []).filter((a) => episode === null || a.episode === episode));
  const roles = $derived((stats?.roles ?? []).filter((r) => scenes.some((s) => s.present.includes(r.cue))));
  const totalMinutes = $derived(scenes.reduce((t, s) => t + s.minutes, 0));
  const totalPages = $derived(eighthsLabel(scenes.reduce((t, s) => t + s.eighths, 0)));
  const showDoubling = $derived(!screen || (stats?.roles ?? []).some((r) => r.track));

  function eighthsLabel(e: number): string {
    const w = Math.floor(e / 8), p = e % 8;
    return w && p ? `${w} ${p}/8` : w ? `${w}` : `${p}/8`;
  }

  const fmt = (m: number) => (m < 1 ? "<1" : Math.round(m).toString());
  const maxMin = $derived(Math.max(1, ...scenes.map((s) => s.minutes)));
  const maxSpeeches = $derived(Math.max(1, ...scenes.flatMap((s) => Object.values(s.speeches))));
  const labelOf = (id: string) => stats?.scenes.find((s) => s.id === id)?.label ?? id;
  const showLabel = (s: StatsScene) => (episode !== null ? s.label.replace(/^E\d+ /, "") : s.label);

  const clashes = $derived(
    new Set((stats?.issues ?? []).flatMap((i) => (i.kind === "clash" ? i.roles.map((r) => `${i.scene}|${r}`) : []))),
  );

  /** One row per scene heading (a scene file can hold several). */
  const breakdown = $derived(
    scenes.flatMap((s) =>
      (s.headings.length ? s.headings : [{ text: "", setting: null, location: "(no heading)", time: null }]).map((h, i) => ({
        scene: s,
        first: i === 0,
        h,
      })),
    ),
  );
  const locations = $derived(
    [...new Set(breakdown.map((b) => b.h.location).filter((l) => l && l !== "(no heading)"))].sort(),
  );
</script>

<div class="prod">
  <div class="head">
    <h2>Production</h2>
    {#if stats?.series}
      <select bind:value={episode} aria-label="Episode">
        <option value={null}>Whole series</option>
        {#each stats.episodes as e}<option value={e.number}>Episode {e.number}: {e.title}</option>{/each}
      </select>
    {/if}
    <button class="ghost" onclick={load}>Refresh</button>
  </div>
  {#if err}<p class="err">{err}</p>{/if}

  {#if check}
    <section aria-label="Script check" data-testid="script-check">
      <h3>
        Script check
        <span class="muted">
          · {#if counted.warnings || counted.infos}{counted.warnings} {counted.warnings === 1 ? "warning" : "warnings"},
            {counted.infos} {counted.infos === 1 ? "note" : "notes"}{:else}nothing to report{/if}
        </span>
        <button class="ghost small" onclick={() => (showSettings = !showSettings)} aria-expanded={showSettings}>Checks…</button>
      </h3>
      <p class="muted small">
        Warnings are probably mistakes (the script reads differently from what you meant); notes are conventions. The same
        checks underline the script as you write.
      </p>
      {#if showSettings}
        <ul class="settings">
          {#each check.checks as c (c.code)}
            <li>
              <label>
                <input type="checkbox" checked={c.on} onchange={(e) => app.setCheck(c.code, e.currentTarget.checked)} />
                {c.name}
              </label>
            </li>
          {/each}
        </ul>
      {/if}
      {#each byScene as g (g.scene)}
        <div class="check-scene">
          <div class="check-head"><strong>{g.label}</strong> {g.title}</div>
          <ul>
            {#each g.issues as i}
              <li class={i.severity}>
                <button
                  class="ghost link"
                  onclick={() => (i.scene ? app.showLine(i.scene, i.line) : (app.view = { kind: "file", name: "project.yaml" }))}
                  title={i.scene ? "Show in the script" : "Open project.yaml"}
                >
                  <span class="sev">{i.severity === "warning" ? "⚠" : "•"}</span>
                  <span class="msg">{i.message}</span>
                  <span class="line muted">{i.scene ? (i.text.trim() ? i.text.trim() : "(blank line)") : "project.yaml"}</span>
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </section>
  {/if}

  {#if stats}
    <section>
      <h3>
        Running time <span class="muted">· {fmt(totalMinutes)} min{screen ? ` · ${totalPages} ${totalPages === "1" ? "page" : "pages"}` : ""}</span>
      </h3>
      <p class="muted small">
        {#if screen}
          Estimated from printed pages ({stats.format === "tv-multi" ? "a page is about 30 seconds" : "a page is about a minute"}),
          unless a scene has a manual duration (*). Pages are counted in eighths, as in a breakdown.
        {:else}
          Estimated at {stats.running_time.words_per_minute} spoken words a minute plus
          {stats.running_time.seconds_per_lyric_line}s per lyric line, unless a scene has a manual duration (*).
          Stage business is not included.
        {/if}
      </p>
      <div class="acts">
        {#each acts as a}<span><strong>{stats.series && episode === null ? `E${a.episode} ` : ""}{a.title}</strong> {fmt(a.minutes)} min{screen ? ` · ${a.pages} pp` : ""}</span>{/each}
      </div>
      <table class="times">
        <thead>
          <tr>
            <th>Scene</th><th>Title</th><th class="n">Min</th><th class="bar-col"></th>
            {#if screen}<th class="n">Pages</th>{:else}<th class="n">Words</th>{/if}
            <th>Songs</th>
          </tr>
        </thead>
        <tbody>
          {#each scenes as s}
            <tr>
              <td><button class="ghost link" onclick={() => app.openScene(s.id)}>{showLabel(s)}</button></td>
              <td>{s.title}</td>
              <td class="n">{fmt(s.minutes)}{s.manual_duration ? "*" : ""}</td>
              <td class="bar-col"><span class="bar" class:manual={s.manual_duration} style="width:{(s.minutes / maxMin) * 100}%"></span></td>
              {#if screen}<td class="n">{s.pages}</td>{:else}<td class="n">{s.words.toLocaleString()}</td>{/if}
              <td class="songs">{s.songs.join(", ")}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>

    {#if screen}
      <section>
        <h3>Scene breakdown <span class="muted">· {locations.length} {locations.length === 1 ? "location" : "locations"}</span></h3>
        <p class="muted small">Each scene heading with its setting, time of day, length and cast — the start of a shooting schedule.</p>
        <table class="breakdown">
          <thead>
            <tr><th>Scene</th><th>Int/Ext</th><th>Location</th><th>Time</th><th class="n">Pages</th><th>Cast</th></tr>
          </thead>
          <tbody>
            {#each breakdown as b}
              <tr class:cont={!b.first}>
                <td>{#if b.first}<button class="ghost link" onclick={() => app.openScene(b.scene.id)}>{showLabel(b.scene)}</button>{/if}</td>
                <td><span class="tag" class:ext={b.h.setting?.startsWith("EXT")}>{b.h.setting ?? "—"}</span></td>
                <td>{b.h.location}</td>
                <td><span class="time" class:night={/NIGHT|DUSK|EVENING/.test(b.h.time ?? "")}>{b.h.time ?? "—"}</span></td>
                <td class="n">{b.first ? b.scene.pages : ""}</td>
                <td class="cast">{b.first ? b.scene.present.join(", ") : ""}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </section>
    {/if}

    <section>
      <h3>Cast by scene</h3>
      <p class="muted small">Number of speeches; • {screen ? "in the scene" : "on stage"} but silent{showDoubling ? ". Red marks a doubling clash." : "."}</p>
      <div class="scroll">
        <table class="matrix">
          <thead>
            <tr>
              <th class="role">Role</th>
              {#each scenes as s}<th class="c"><button class="ghost link" onclick={() => app.openScene(s.id)} title={s.title}>{showLabel(s)}</button></th>{/each}
              {#if showDoubling}<th class="track">Track</th>{/if}
            </tr>
          </thead>
          <tbody>
            {#each roles as r}
              <tr>
                <th class="role">{r.cue}</th>
                {#each scenes as s}
                  {@const n = s.speeches[r.cue]}
                  {@const here = s.present.includes(r.cue)}
                  <td
                    class="c"
                    class:clash={clashes.has(`${s.id}|${r.cue}`)}
                    style={n ? `--w:${0.12 + 0.6 * (n / maxSpeeches)}` : ""}
                    class:filled={!!n}
                  >{n ?? (here ? "•" : "")}</td>
                {/each}
                {#if showDoubling}<td class="track muted">{r.track ?? "—"}</td>{/if}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    {#if showDoubling}
      <section>
        <h3>Doubling</h3>
        {#if stats.issues.length === 0}
          <p>No problems found.</p>
        {:else}
          <ul class="issues">
            {#each stats.issues as i}
              {#if i.kind === "clash"}
                <li class="clash"><strong>Clash</strong> in {labelOf(i.scene)}: {i.roles.join(" and ")} are both played by {i.track}.</li>
              {/if}
            {/each}
            {#each stats.issues as i}
              {#if i.kind === "quick_change"}
                <li class="quick"><strong>Quick change</strong> {labelOf(i.from_scene)} → {labelOf(i.to_scene)}: {i.track} goes from {i.from_role} to {i.to_role}.</li>
              {/if}
            {/each}
            {#each stats.issues as i}
              {#if i.kind === "unassigned"}
                <li class="unassigned"><strong>No actor</strong> for {i.role} ({i.scenes.map(labelOf).join(", ")}). Set a track in characters.yaml.</li>
              {/if}
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  {:else if !err}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .prod {
    height: 100%;
    overflow: auto;
    padding: 20px 28px 60px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .head select {
    width: auto;
  }
  h2 {
    margin: 0;
    font-size: 20px;
  }
  h3 {
    font-size: 15px;
    margin: 28px 0 4px;
  }
  .small {
    font-size: 12.5px;
    margin: 0 0 10px;
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 20px;
    margin-bottom: 8px;
    font-size: 13px;
  }
  table {
    border-collapse: collapse;
    font-size: 13px;
  }
  th,
  td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--line);
    text-align: left;
  }
  thead th {
    font-size: 11.5px;
    color: var(--ink-3);
    font-weight: 600;
  }
  .n {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .times,
  .breakdown {
    width: 100%;
    max-width: 960px;
  }
  .breakdown tr.cont td {
    border-top: 0;
  }
  .tag {
    font: 600 11px var(--ui-font);
    padding: 1px 6px;
    border-radius: 99px;
    background: var(--panel);
    border: 1px solid var(--line);
  }
  .tag.ext {
    background: color-mix(in srgb, var(--song) 15%, transparent);
    border-color: var(--song);
  }
  .time.night {
    color: var(--song);
    font-weight: 600;
  }
  .cast {
    color: var(--ink-2);
    font-size: 12px;
  }
  .bar-col {
    width: 30%;
  }
  .bar {
    display: block;
    height: 8px;
    border-radius: 4px;
    background: var(--accent);
    opacity: 0.7;
  }
  .bar.manual {
    background: var(--song);
  }
  .songs {
    color: var(--song);
  }
  .link {
    padding: 0 4px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .scroll {
    overflow-x: auto;
  }
  .matrix .c {
    text-align: center;
    min-width: 2.6em;
    font-variant-numeric: tabular-nums;
    border-left: 1px solid var(--line);
  }
  .matrix td.filled {
    background: color-mix(in srgb, var(--accent) calc(var(--w) * 100%), transparent);
  }
  .matrix td.clash {
    outline: 2px solid var(--bad);
    outline-offset: -2px;
    font-weight: 700;
  }
  .matrix .role {
    font-weight: 600;
    white-space: nowrap;
  }
  .track {
    white-space: nowrap;
  }
  .issues {
    padding-left: 18px;
  }
  .issues li {
    margin-bottom: 4px;
  }
  .clash strong {
    color: var(--bad);
  }
  .quick strong {
    color: var(--warn);
  }
  .unassigned strong {
    color: var(--ink-2);
  }
  .err {
    color: var(--bad);
  }
  .settings {
    list-style: none;
    padding: 6px 0;
    margin: 0 0 8px;
    columns: 2;
    font-size: 13px;
  }
  .settings li {
    padding: 2px 0;
  }
  .check-scene {
    margin: 6px 0 10px;
    max-width: 960px;
  }
  .check-head {
    font-size: 13px;
    margin-bottom: 2px;
  }
  .check-scene ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .check-scene li .link {
    white-space: normal;
    align-items: baseline;
    display: grid;
    grid-template-columns: 1.4em minmax(0, 1fr) minmax(0, 0.8fr);
    gap: 8px;
    width: 100%;
    text-align: left;
    padding: 3px 6px;
    font-size: 13px;
    border-radius: 4px;
  }
  .check-scene li .link:hover {
    background: var(--panel);
  }
  .check-scene .warning .sev {
    color: var(--warn);
  }
  .check-scene .info .sev {
    color: var(--song);
  }
  .check-scene .line {
    font-family: var(--script-font);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
