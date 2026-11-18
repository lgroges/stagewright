<!-- The AI review's settings, in Production → Checks: which AI and model (this
     computer's), the Claude API key, and whether the project may send its
     scenes to Anthropic (the project's). -->
<script lang="ts">
  import { onMount } from "svelte";
  import { CLAUDE_MODELS } from "../lib/ai.svelte";
  import { ai, type AiStatus } from "../lib/api";
  import { app } from "../lib/state.svelte";

  // A copy to edit; saved as each field changes.
  let s = $state({ ...app.ai.settings });
  let status = $state<AiStatus | null>(null);
  let keyDraft = $state("");
  let keyError = $state("");

  async function check() {
    app.ai.save(s);
    s = { ...app.ai.settings };
    status = null;
    status = await ai.status(s.provider, s.url);
  }

  async function saveKey(key: string) {
    keyError = "";
    try {
      await ai.setKey(key);
      keyDraft = "";
      await check();
    } catch (e) {
      keyError = String((e as Error)?.message ?? e);
    }
  }

  onMount(check);
  const models = $derived(status?.models ?? []);
</script>

<div class="ai-settings" data-testid="ai-settings">
  <strong>AI review</strong>
  <span class="muted small">
    Review scene, under the script, asks an AI for grammar and wrong words the checks miss. These settings are this
    computer's.
  </span>
  <label>
    Use
    <select bind:value={s.provider} onchange={check} data-testid="ai-provider">
      <option value="ollama">Ollama, on this computer</option>
      <option value="claude">Claude (Anthropic)</option>
    </select>
  </label>
  {#if s.provider === "ollama"}
    <label>
      Ollama
      <input bind:value={s.url} onchange={check} spellcheck="false" data-testid="ai-url" />
    </label>
    <label>
      Model
      <select bind:value={s.model} onchange={() => app.ai.save(s)} data-testid="ai-model">
        <option value="">The best installed (Qwen 3.6, Qwen 3 …)</option>
        {#each models as m (m)}<option value={m}>{m}</option>{/each}
        {#if s.model && !models.includes(s.model)}<option value={s.model}>{s.model}</option>{/if}
      </select>
    </label>
    <span class="small" class:bad={status && !status.ok} data-testid="ai-reachable">
      {#if !status}Checking…{:else if status.ok}Ollama is running ({models.length}
        {models.length === 1 ? "model" : "models"}). The script isn't sent anywhere else.{:else}{status.reason}{/if}
    </span>
  {:else}
    <label>
      Model
      <select bind:value={s.claudeModel} onchange={() => app.ai.save(s)} data-testid="ai-claude-model">
        {#each CLAUDE_MODELS as m (m.id)}<option value={m.id}>{m.name} ({m.cost})</option>{/each}
      </select>
    </label>
    <label>
      API key
      <input
        type="password"
        bind:value={keyDraft}
        placeholder={status?.has_key ? "saved (type to replace)" : "sk-ant-…"}
        autocomplete="off"
        spellcheck="false"
        data-testid="ai-key"
      />
    </label>
    <button class="ghost" disabled={!keyDraft.trim()} onclick={() => saveKey(keyDraft)}>Save key</button>
    {#if status?.stored}<button class="ghost" onclick={() => saveKey("")}>Remove key</button>{/if}
    <span class="small" class:bad={(status && !status.ok) || keyError} data-testid="ai-reachable">
      {#if keyError}{keyError}{:else if !status}Checking…{:else if status.ok}The key works{status.stored
          ? ". It's kept in this computer's credential store, not in the project."
          : " (from ANTHROPIC_API_KEY)."}{:else}{status.reason}{/if}
    </span>
    <label class="consent" data-testid="ai-cloud">
      <input type="checkbox" checked={app.project?.ai_cloud} onchange={(e) => app.ai.allowCloud(e.currentTarget.checked)} />
      This project may send its scenes to Anthropic for review{app.collab ? " (co-written: check with your co-writers)" : ""}
    </label>
  {/if}
</div>

<style>
  .ai-settings {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    margin: 0 0 12px;
    padding: 8px 10px;
    font-size: 13px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    max-width: 960px;
  }
  .ai-settings > .muted {
    flex-basis: 100%;
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  input:not([type="checkbox"]) {
    width: 220px;
  }
  .consent {
    flex-basis: 100%;
    justify-content: flex-start;
  }
  .bad {
    color: var(--bad);
  }
</style>
