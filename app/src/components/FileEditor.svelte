<script lang="ts">
  import { onMount } from "svelte";
  import { EditorView, keymap } from "@codemirror/view";
  import { EditorState } from "@codemirror/state";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { yaml } from "@codemirror/lang-yaml";
  import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
  import { api, ApiError } from "../lib/api";
  import { app } from "../lib/state.svelte";

  let { name }: { name: string } = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let version = "";
  let dirty = $state(false);
  let status = $state<string | null>(null);
  let problem = $state<string | null>(null);

  const help: Record<string, string> = {
    "project.yaml": "Title, authors, rating names, running-time settings and act titles.",
    "characters.yaml": "One entry per character: id, cue (as written in the script), name, track (who plays it).",
    "locations.yaml": "Places scenes can be set: id and name.",
    "songs.yaml": "The musical numbers.",
  };

  async function save() {
    if (!view) return;
    try {
      const text = view.state.doc.toString();
      const r = await api.fileSave(name, text, version);
      version = r.version;
      app.collab?.setFile(name, text);
      dirty = false;
      problem = null;
      status = "Saved";
      await app.refreshProject();
    } catch (e) {
      problem = e instanceof ApiError ? `${e.code === "invalid_yaml" ? "Not saved — " : ""}${e.message}` : String(e);
      status = null;
    }
  }

  onMount(() => {
    let cancelled = false;
    (async () => {
      try {
        const f = await api.fileRead(name);
        if (cancelled) return;
        version = f.version;
        view = new EditorView({
          parent: host,
          state: EditorState.create({
            doc: f.text,
            extensions: [
              history(),
              yaml(),
              syntaxHighlighting(defaultHighlightStyle),
              EditorView.lineWrapping,
              keymap.of([{ key: "Mod-s", run: () => (void save(), true), preventDefault: true }, ...historyKeymap, ...defaultKeymap]),
              EditorView.updateListener.of((u) => {
                if (u.docChanged) {
                  dirty = true;
                  status = null;
                }
              }),
              EditorView.theme({
                "&": { height: "100%" },
                ".cm-scroller": { fontFamily: "ui-monospace, Consolas, monospace", fontSize: "13px" },
                ".cm-content": { padding: "16px 0" },
              }),
            ],
          }),
        });
      } catch (e) {
        problem = e instanceof Error ? e.message : String(e);
      }
    })();
    return () => {
      cancelled = true;
      view?.destroy();
    };
  });
</script>

<div class="file">
  <div class="bar">
    <strong>{name}</strong>
    <span class="muted">{help[name] ?? ""}</span>
    <span class="status">{dirty ? "Edited" : (status ?? "")}</span>
    <button class="primary" onclick={save} disabled={!dirty}>Save</button>
  </div>
  {#if problem}<div class="problem" role="alert">{problem}</div>{/if}
  <div class="host" bind:this={host}></div>
</div>

<style>
  .file {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .bar {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 8px 14px;
    border-bottom: 1px solid var(--line);
  }
  .bar .muted {
    font-size: 12.5px;
    flex: 1;
  }
  .status {
    font-size: 12px;
    color: var(--ink-3);
  }
  .problem {
    padding: 8px 14px;
    color: var(--bad);
    background: var(--accent-soft);
    font-size: 13px;
    white-space: pre-wrap;
  }
  .host {
    flex: 1;
    min-height: 0;
  }
</style>
