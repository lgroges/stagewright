<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { app } from "../lib/state.svelte";
  import { scriptExtensions, ELEMENT_KEYS, applyElement } from "../lib/editor/setup";
  import { info, toolbarElement, stanzaBreak, asCommand, type ElementType } from "../lib/editor/script";
  import { liveStats } from "../lib/classify";
  import { openLintPanel } from "@codemirror/lint";
  import { recheck } from "../lib/editor/check";
  import { refreshContd, setSpelling } from "../lib/editor/setup";
  import { api, type Comment } from "../lib/api";
  import CommentsPanel from "./CommentsPanel.svelte";
  import {
    anchorAt,
    commentSpans,
    locate,
    setActiveComment,
    setCommentMarks,
    setShowResolved,
    type CommentMark,
    type Span,
  } from "../lib/editor/comments";

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
  let issues = $state({ warnings: 0, infos: 0 });
  let hasSelection = $state(false);
  /** Where each of this scene's comments is now (absent: its text was deleted). */
  let spans = $state<Map<string, Span>>(new Map());
  let draft = $state<{ from: number; to: number; quote: string } | null>(null);
  /** Each comment's stored anchor as last placed or saved here, to tell our own saves from others' changes. */
  const placed = new Map<string, string>();
  let anchorTimer: ReturnType<typeof setTimeout> | null = null;

  const anchorKey = (a: { quote: string; before: string; after: string; line: number }) =>
    JSON.stringify([a.quote, a.before, a.after, a.line]);

  /**
   * Highlight this scene's comments. A comment whose stored anchor is the one
   * placed here keeps its live highlight (which has followed the edits); any
   * other (new, or moved by a co-writer, or `fresh` after loading) is located.
   */
  function placeComments(fresh = false) {
    if (!view || !shownId) return;
    const doc = view.state.doc.toString();
    const live = commentSpans(view.state);
    const marks: CommentMark[] = [];
    for (const c of app.comments) {
      if (c.scene !== shownId) continue;
      const key = anchorKey(c);
      const span = !fresh && placed.get(c.id) === key ? (live.get(c.id) ?? null) : locate(doc, c);
      placed.set(c.id, key);
      if (span) marks.push({ id: c.id, from: span.from, to: span.to, resolved: c.resolved });
    }
    const [active, showResolved] = untrack(() => [app.activeComment, app.showResolved] as const);
    view.dispatch({ effects: [setCommentMarks.of(marks), setActiveComment.of(active), setShowResolved.of(showResolved)] });
    spans = commentSpans(view.state);
  }

  /** Store where comments are now, for those whose passage has moved or changed. */
  function saveAnchors() {
    if (anchorTimer) clearTimeout(anchorTimer);
    anchorTimer = null;
    if (!view || !shownId) return;
    const doc = view.state.doc.toString();
    const live = commentSpans(view.state);
    const moved = [];
    for (const c of app.comments) {
      const s = c.scene === shownId ? live.get(c.id) : undefined;
      if (!s) continue;
      const a = anchorAt(doc, s.from, s.to);
      if (anchorKey(a) !== anchorKey(c)) {
        placed.set(c.id, anchorKey(a));
        moved.push({ id: c.id, ...a });
      }
    }
    if (moved.length) void app.editComments({ op: "anchor", anchors: moved });
  }

  function newComment() {
    if (!view) return;
    const sel = view.state.selection.main;
    if (sel.empty) return;
    draft = { from: sel.from, to: sel.to, quote: view.state.sliceDoc(sel.from, sel.to) };
    app.activeComment = null;
    app.toggleComments(true);
  }

  async function submitDraft(text: string): Promise<boolean> {
    if (!view || !draft || !shownId) return false;
    // The draft's passage may have moved while it was typed: map it to now.
    const doc = view.state.doc.toString();
    const span: Span | null =
      doc.slice(draft.from, draft.to) === draft.quote ? draft : locate(doc, { quote: draft.quote, before: "", after: "", line: 0 });
    if (!span) {
      app.error = "The text you were commenting on has changed. Select it again.";
      return false;
    }
    const id = "cm-" + Math.random().toString(36).slice(2, 10);
    const ok = await app.editComments({
      op: "add",
      id,
      scene: shownId,
      ...anchorAt(doc, span.from, span.to),
      author: app.author,
      at: new Date().toISOString(),
      text,
    });
    if (ok) {
      draft = null;
      app.activeComment = id;
    }
    return ok;
  }

  /** Show a comment's passage in the script. */
  function selectComment(id: string) {
    app.activeComment = id;
    const s = view && commentSpans(view.state).get(id);
    if (view && s) view.dispatch({ selection: { anchor: s.from, head: s.to }, scrollIntoView: true });
  }

  const sceneComments = $derived.by(() => {
    const id = app.scene?.id;
    const list: { comment: Comment; at: number | null }[] = app.comments
      .filter((c) => c.scene === id)
      .map((c) => ({ comment: c, at: spans.get(c.id)?.from ?? null }));
    // In script order; detached ones last.
    return list.sort((a, b) => (a.at ?? Infinity) - (b.at ?? Infinity));
  });
  const openCount = $derived(sceneComments.filter((c) => !c.comment.resolved).length);

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
          names: () =>
            (app.project?.characters ?? []).flatMap((c) => [c.short, c.name].filter((n): n is string => !!n && /\p{Ll}/u.test(n))),
        },
        onChange: (s) => {
          app.bodyChanged(s.doc.toString());
          measure(s);
          spans = commentSpans(s);
          if (anchorTimer) clearTimeout(anchorTimer);
          anchorTimer = setTimeout(saveAnchors, 1200);
        },
        onCursor: (s) => {
          active = toolbarElement(s);
          hasSelection = !s.selection.main.empty;
        },
        onComment: (id) => {
          app.activeComment = id;
          if (id) app.toggleComments(true);
        },
        onNewComment: newComment,
        onSave: () => void app.flush(),
        check: {
          check: async (body) => {
            const sc = app.scene;
            return sc ? (await api.checkScene(sc.id, body)).issues : null;
          },
          off: (code) => void app.setCheck(code, false),
        },
        autoContd: () => app.project?.auto_contd ?? false,
        spellcheck: app.spellcheck,
        onIssues: (n) => {
          if (n.warnings !== issues.warnings || n.infos !== issues.infos) issues = n;
        },
      }),
    });
  }

  function show() {
    const sc = app.scene;
    if (!view || !sc) return;
    if (sc.loadToken === shownToken) return;
    // Store where the last scene's comments ended up before leaving it.
    saveAnchors();
    draft = null;
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
    issues = { warnings: 0, infos: 0 };
    shownToken = sc.loadToken;
    shownId = sc.id;
    placeComments(true);
    measure(state);
    active = toolbarElement(state);
    view.focus();
  }

  onMount(() => {
    view = new EditorView({ parent: host, state: makeState("") });
    // For end-to-end tests: the editor only renders visible lines, so expose the full text.
    (window as unknown as { __swText?: () => string }).__swText = () => view?.state.doc.toString() ?? "";
    // …and select a passage by its text (the first match).
    (window as unknown as { __swSelect?: (t: string) => boolean }).__swSelect = (t: string) => {
      const at = view?.state.doc.toString().indexOf(t) ?? -1;
      if (!view || at < 0) return false;
      view.dispatch({ selection: { anchor: at, head: at + t.length }, scrollIntoView: true });
      view.focus();
      return true;
    };
    show();
    return () => {
      saveAnchors();
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

  $effect(() => {
    void app.goTo;
    void app.scene?.loadToken;
    jump();
  });

  // Check again when the check settings or the cast list change.
  $effect(() => {
    void app.checksVersion;
    void app.project?.cues;
    if (view) view.dispatch({ effects: recheck.of(null) });
  });

  // Comments changed (here, by a co-writer, or in comments.yaml): place them again.
  $effect(() => {
    void app.comments;
    void app.scene?.loadToken;
    placeComments();
  });

  $effect(() => {
    const id = app.activeComment;
    const on = app.showResolved;
    if (view) view.dispatch({ effects: [setActiveComment.of(id), setShowResolved.of(on)] });
  });

  $effect(() => {
    const on = app.spellcheck;
    if (view) view.dispatch({ effects: setSpelling.of(on) });
  });

  // Show or hide the automatic (CONT'D) when the setting changes.
  $effect(() => {
    void app.project?.auto_contd;
    if (view) view.dispatch({ effects: refreshContd.of(null) });
  });

  /** Go to a line asked for elsewhere (the Production issue list). */
  function jump() {
    const g = app.goTo;
    if (!view || !g || g.id !== app.scene?.id) return;
    app.goTo = null;
    const n = Math.min(g.line + 1, view.state.doc.lines);
    const line = view.state.doc.line(n);
    view.dispatch({ selection: { anchor: line.from, head: line.to }, scrollIntoView: true });
    view.focus();
  }

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
    <button
      class="ghost comment-add"
      title="Comment on the selected text (Ctrl+Alt+M)"
      onmousedown={(e) => e.preventDefault()}
      onclick={newComment}
      disabled={!hasSelection}
      data-testid="comment-add">Comment</button
    >
    <button
      class="ghost"
      class:on={app.commentsOpen}
      onclick={() => app.toggleComments()}
      aria-pressed={app.commentsOpen}
      title={app.commentsOpen ? "Hide comments" : "Show comments"}
      data-testid="comments-toggle">💬 {openCount}</button
    >
    <button
      class="ghost spell"
      class:off={!app.spellcheck}
      onclick={() => app.toggleSpellcheck()}
      aria-pressed={app.spellcheck}
      title={app.spellcheck ? "Spelling is checked (your computer's dictionary). Click to stop." : "Spelling isn't checked. Click to check it."}
      data-testid="spellcheck">Spelling</button
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

  <div class="work">
    <div class="cm-host fmt-{format}" bind:this={host}></div>
    {#if app.commentsOpen}
      <CommentsPanel shown={sceneComments} {draft} onselect={selectComment} onsubmit={submitDraft} oncancel={() => (draft = null)} />
    {/if}
  </div>

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
    {#if issues.warnings || issues.infos}
      <button
        class="ghost issues"
        class:warn={issues.warnings > 0}
        title="Show the script check's notes for this scene (F8 goes to the next one)"
        onclick={() => view && openLintPanel(view)}
        data-testid="issue-count"
      >
        {#if issues.warnings}⚠ {issues.warnings} {issues.warnings === 1 ? "warning" : "warnings"}{/if}
        {#if issues.warnings && issues.infos}·{/if}
        {#if issues.infos}{issues.infos} {issues.infos === 1 ? "note" : "notes"}{/if}
      </button>
    {/if}
    <span class="file muted">{app.scene?.summary.file}</span>
  </footer>
</div>

<style>
  .comment-add {
    margin-left: auto;
  }
  .spell {
    text-decoration: underline wavy color-mix(in srgb, var(--bad) 70%, transparent);
    text-underline-offset: 4px;
  }
  .spell.off {
    text-decoration: none;
    color: var(--ink-3);
  }
  .spell + .keys {
    margin-left: 0;
  }
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
  .work {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .cm-host {
    flex: 1;
    min-width: 0;
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
  .issues {
    padding: 0 6px;
    font-size: 12px;
    color: var(--song);
  }
  .issues.warn {
    color: var(--warn);
    font-weight: 600;
  }
  .file {
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
