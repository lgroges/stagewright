<script lang="ts">
  import { onMount } from "svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { app } from "../lib/state.svelte";
  import { scriptExtensions, ELEMENT_KEYS, applyElement } from "../lib/editor/setup";
  import { info, toolbarElement, stanzaBreak, asCommand, type ElementType } from "../lib/editor/script";
  import { liveStats } from "../lib/classify";

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  const cache = new Map<string, EditorState>();
  let shownToken = -1;
  let shownId: string | null = null;

  let active = $state<ElementType | null>(null);
  let words = $state(0);
  let lyricLines = $state(0);
  let speakers = $state(0);
  let showKeys = $state(false);

  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  const keyLabel = (k: string) => k.replace("Mod-", isMac ? "⌘" : "Ctrl+").replace("Shift-", isMac ? "⇧" : "Shift+");

  function measure(state: EditorState) {
    const lines: string[] = [];
    for (let i = 1; i <= state.doc.lines; i++) lines.push(state.doc.line(i).text);
    const s = liveStats(lines, info(state).kinds);
    words = s.words;
    lyricLines = s.lyricLines;
    speakers = s.speakers.size;
  }

  function sharedFor(id: string) {
    const c = app.collab;
    if (!c) return null;
    const text = c.sceneText(id);
    const undoManager = c.undoManager(id);
    return text && undoManager ? { text, awareness: c.provider.awareness, undoManager } : null;
  }

  function makeState(body: string, id: string | null = null): EditorState {
    const shared = id ? sharedFor(id) : null;
    return EditorState.create({
      doc: shared ? shared.text.toString() : body,
      extensions: scriptExtensions({
        shared,
        cues: {
          cues: () => app.project?.cues ?? [],
          places: () => app.project?.heading_locations ?? [],
          screen: () => (app.project?.format ?? "stage") !== "stage",
        },
        onChange: (s) => {
          app.bodyChanged(s.doc.toString());
          measure(s);
        },
        onCursor: (s) => (active = toolbarElement(s)),
        onSave: () => void app.flush(),
      }),
    });
  }

  function show() {
    const sc = app.scene;
    if (!view || !sc) return;
    if (sc.loadToken === shownToken) return;
    let state: EditorState;
    if (app.collab) {
      // Shared text: always bind afresh (undo history lives in the shared session).
      cache.clear();
      state = makeState("", sc.id);
    } else {
      if (shownId) cache.set(shownId, view.state);
      const cached = cache.get(sc.id);
      // Reuse the cached state (keeps undo history) only if it holds the latest text.
      state = cached && cached.doc.toString() === sc.body ? cached : makeState(sc.body);
    }
    view.setState(state);
    shownToken = sc.loadToken;
    shownId = sc.id;
    measure(state);
    active = toolbarElement(state);
    view.focus();
  }

  onMount(() => {
    view = new EditorView({ parent: host, state: makeState("") });
    // For end-to-end tests: the editor only renders visible lines, so expose the full text.
    (window as unknown as { __swText?: () => string }).__swText = () => view?.state.doc.toString() ?? "";
    show();
    return () => {
      void app.flush();
      view?.destroy();
      view = null;
    };
  });

  $effect(() => {
    // re-run when the open scene or its load token changes
    void app.scene?.id;
    void app.scene?.loadToken;
    show();
  });

  const minutes = $derived.by(() => {
    const rt = app.project?.running_time ?? { words_per_minute: 150, seconds_per_lyric_line: 6 };
    return (words / rt.words_per_minute) + (lyricLines * rt.seconds_per_lyric_line) / 60;
  });

  const format = $derived(app.project?.format ?? "stage");
  const screen = $derived(format !== "stage");
  /** Toolbar order: screen formats lead with the scene heading and call directions "Action". */
  const tools = $derived(
    screen
      ? [ELEMENT_KEYS.find((k) => k.el === "heading")!, ...ELEMENT_KEYS.filter((k) => k.el !== "heading")]
      : ELEMENT_KEYS.filter((k) => k.el !== "heading"),
  );
  const toolLabel = (el: ElementType, label: string) => (el === "direction" && screen ? "Action" : label);

  const elementName: Record<ElementType, string> = {
    heading: "Scene heading",
    direction: "Direction",
    cue: "Character",
    parenthetical: "Parenthetical",
    dialogue: "Dialogue",
    lyric: "Lyric",
    song: "Song",
    transition: "Transition",
  };

  function press(el: ElementType) {
    if (view) applyElement(view, el);
  }
</script>

<div class="editor">
  <div class="toolbar" role="toolbar" aria-label="Script elements">
    {#each tools as { el, key, label }}
      <button
        class="ghost"
        class:on={active === el}
        title="{toolLabel(el, label)} ({keyLabel(key)})"
        onmousedown={(e) => e.preventDefault()}
        onclick={() => press(el)}
        data-element={el}>{toolLabel(el, label)}</button
      >
    {/each}
    <button
      class="ghost"
      title="Stanza break ({keyLabel('Mod-Shift-L')})"
      onmousedown={(e) => e.preventDefault()}
      onclick={() => view && asCommand(stanzaBreak)(view)}
      disabled={active !== "lyric" && active !== "song"}>Stanza</button
    >
    <button class="ghost keys" onclick={() => (showKeys = !showKeys)} aria-expanded={showKeys}>Keys</button>
  </div>

  {#if showKeys}
    <div class="keyhelp">
      {#if screen}
        <div><kbd>{keyLabel("Mod-8")}</kbd> starts a scene heading: pick INT./EXT., a place you've used, then a time of day.</div>
      {/if}
      <div><kbd>Tab</kbd> on an empty line starts a speech. On dialogue, toggles a parenthetical.</div>
      <div><kbd>Enter</kbd> moves to what usually comes next: character → dialogue → new paragraph.</div>
      <div><kbd>Shift+Enter</kbd> plain new line. <kbd>Shift+Tab</kbd> steps an element back.</div>
      <div>In a song, <kbd>Enter</kbd> on an empty lyric line ends the song.</div>
      <div><kbd>{keyLabel("Mod-S")}</kbd> saves now (it also saves as you type). <kbd>{keyLabel("Mod-F")}</kbd> finds.</div>
    </div>
  {/if}

  {#if app.saveStatus === "conflict"}
    <div class="conflict" role="alert">
      <strong>This scene was changed outside Stagewright.</strong>
      <span>Your edits are not saved yet.</span>
      <button onclick={() => app.reloadFromDisk()}>Load the other version</button>
      <button class="primary" onclick={() => app.keepMine()}>Keep mine</button>
    </div>
  {/if}

  <div class="cm-host fmt-{format}" bind:this={host}></div>

  <footer class="status">
    <span data-testid="element">{active ? (active === "direction" && screen ? "Action" : elementName[active]) : "—"}</span>
    <span>{words.toLocaleString()} words</span>
    <span>{speakers} speaking</span>
    {#if screen}
      <span title="Printed length (updates when the scene is saved); a page is about {format === 'tv-multi' ? 'half a minute' : 'a minute'}">
        {app.scene?.summary.pages || "–"} pp · ≈ {(app.scene?.summary.minutes ?? 0).toFixed(1)} min
      </span>
    {:else}
      <span title="Estimated from spoken words{app.scene?.meta.duration ? '; a manual duration is set in Details' : ''}">
        ≈ {minutes.toFixed(1)} min{app.scene?.meta.duration ? ` (set: ${app.scene.meta.duration})` : ""}
      </span>
    {/if}
    <span class="file muted">{app.scene?.summary.file}</span>
  </footer>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--line);
    background: var(--paper);
  }
  .toolbar button {
    font-size: 12.5px;
    padding: 3px 9px;
  }
  .toolbar button.on {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .keys {
    margin-left: auto;
  }
  .keyhelp {
    padding: 10px 16px;
    font-size: 12.5px;
    color: var(--ink-2);
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    display: grid;
    gap: 4px;
  }
  .conflict {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
    padding: 10px 16px;
    background: var(--accent-soft);
    border-bottom: 1px solid var(--line);
  }
  .conflict span {
    flex: 1;
  }
  .cm-host {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .cm-host :global(.cm-editor) {
    height: 100%;
  }
  /* Screenplay layout (film and TV): columns as on the page, at 60 characters. */
  .cm-host:not(.fmt-stage) :global(.cm-content) {
    max-width: 60ch;
  }
  .cm-host:not(.fmt-stage) :global(.sw-direction) {
    font-style: normal;
    color: var(--ink);
  }
  .cm-host:not(.fmt-stage) :global(.sw-cue) {
    text-align: left;
    padding-left: 37%;
    padding-top: 0.6em;
  }
  .cm-host:not(.fmt-stage) :global(.sw-parenthetical) {
    text-align: left;
    padding-left: 27%;
    padding-right: 30%;
    font-style: normal;
  }
  .cm-host:not(.fmt-stage) :global(.sw-dialogue) {
    padding-left: 17%;
    padding-right: 25%;
  }
  .cm-host:not(.fmt-stage) :global(.sw-lyric),
  .cm-host:not(.fmt-stage) :global(.sw-lyric-break) {
    padding-left: 17%;
  }
  /* Multi-camera: action in capitals, dialogue double-spaced, headings underlined. */
  .cm-host.fmt-tv-multi :global(.sw-direction) {
    text-transform: uppercase;
  }
  .cm-host.fmt-tv-multi :global(.sw-dialogue) {
    line-height: 2.4;
  }
  .cm-host.fmt-tv-multi :global(.sw-scene-heading) {
    text-decoration: underline;
  }
  .status {
    display: flex;
    gap: 18px;
    padding: 5px 14px;
    font-size: 12px;
    color: var(--ink-2);
    border-top: 1px solid var(--line);
    background: var(--panel);
    font-variant-numeric: tabular-nums;
  }
  .file {
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
