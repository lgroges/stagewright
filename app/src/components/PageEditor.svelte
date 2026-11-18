<!-- A notes page's text: Markdown, with [[ offering every character, place,
     scene, beat and research note to link to. Changes are passed on a moment
     after typing stops, and on leaving the editor. -->
<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { EditorView, keymap, placeholder as placeholderText } from "@codemirror/view";
  import { EditorState } from "@codemirror/state";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown } from "@codemirror/lang-markdown";
  import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
  import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
  import type { ContextRecord } from "../lib/api";

  let {
    text,
    records,
    placeholder = "",
    onchange,
  }: { text: string; records: ContextRecord[]; placeholder?: string; onchange: (text: string) => void } = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  // The text as last passed on (the editor is made once, from the first `text`).
  let sent = untrack(() => text);

  const kinds: Record<string, string> = {
    character: "character",
    location: "place",
    scene: "scene",
    beat: "beat",
    research: "research",
  };

  /** `[[` then any part of a name or id: the records that match. */
  function links(ctx: CompletionContext) {
    const m = ctx.matchBefore(/\[\[[^\]|\n]*/);
    if (!m) return null;
    return {
      from: m.from + 2,
      options: records.map((r) => ({
        label: r.id,
        displayLabel: r.label ? `${r.label} ${r.name}` : r.name,
        detail: kinds[r.kind],
        apply: `${r.id}]]`,
      })),
      // Match the name as well as the id.
      filter: true,
    };
  }

  function send() {
    if (timer) clearTimeout(timer);
    timer = null;
    const now = view?.state.doc.toString() ?? sent;
    if (now !== sent) {
      sent = now;
      onchange(now);
    }
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: sent,
        extensions: [
          history(),
          markdown(),
          syntaxHighlighting(defaultHighlightStyle),
          EditorView.lineWrapping,
          placeholderText(placeholder),
          autocompletion({ override: [links], activateOnTyping: true }),
          keymap.of([{ key: "Mod-s", run: () => (send(), true), preventDefault: true }, ...historyKeymap, ...defaultKeymap]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              if (timer) clearTimeout(timer);
              timer = setTimeout(send, 800);
            }
            if (u.focusChanged && !u.view.hasFocus) send();
          }),
          EditorView.theme({
            "&": { minHeight: "220px", border: "1px solid var(--line)", borderRadius: "var(--radius)", background: "var(--paper)" },
            "&.cm-focused": { outline: "2px solid color-mix(in srgb, var(--accent) 40%, transparent)" },
            ".cm-scroller": { fontFamily: "var(--ui-font)", fontSize: "14px", lineHeight: "1.55" },
            ".cm-content": { padding: "10px 12px", caretColor: "var(--cursor)" },
          }),
        ],
      }),
    });
    return () => {
      send();
      view?.destroy();
    };
  });
</script>

<div class="page-editor" bind:this={host} data-testid="page-editor"></div>
