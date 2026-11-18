// The AI review in the app: this computer's settings (which AI, which model),
// the latest review's findings per scene, and the review under way.

import { ai, type AiFinding, type AiProvider, type ProjectView } from "./api";
import { readPref, writePref } from "./prefs";

export const DEFAULT_OLLAMA_URL = "http://127.0.0.1:11434";
export const DEFAULT_CLAUDE_MODEL = "claude-sonnet-5-5";

/** The Claude models offered, with a rough cost a scene. */
export const CLAUDE_MODELS = [
  { id: "claude-sonnet-5-5", name: "Sonnet 5.5", cost: "about 1p a scene" },
  { id: "claude-opus-5-5", name: "Opus 5.5", cost: "about 2–3p a scene" },
  { id: "claude-haiku-5-5", name: "Haiku 5.5", cost: "a fraction of 1p" },
];

/** "Sonnet 5.5" for "claude-sonnet-5-5". */
export const claudeName = (id: string) => CLAUDE_MODELS.find((m) => m.id === id)?.name ?? id;

/** "$0.013", or "under $0.01". */
const dollars = (usd: number) => (usd < 0.01 ? "under $0.01" : `$${usd.toFixed(usd < 1 ? 3 : 2)}`);

/** Which AI, and the models: this computer's, not the project's. */
export interface AiSettings {
  provider: AiProvider;
  /** Where Ollama is, and its model ("" = the best installed). */
  url: string;
  model: string;
  claudeModel: string;
}

const PREFS: Record<keyof AiSettings, string> = {
  provider: "stagewright.ai.provider",
  url: "stagewright.ai.url",
  model: "stagewright.ai.model",
  claudeModel: "stagewright.ai.claude-model",
};

function loadSettings(): AiSettings {
  return {
    provider: readPref(PREFS.provider, "ollama") === "claude" ? "claude" : "ollama",
    url: readPref(PREFS.url, DEFAULT_OLLAMA_URL),
    model: readPref(PREFS.model, ""),
    claudeModel: readPref(PREFS.claudeModel, DEFAULT_CLAUDE_MODEL),
  };
}

/** What the AI state needs from the app. */
interface Host {
  project: ProjectView | null;
  scene: { id: string } | null;
  checksVersion: number;
  setOption(name: "ai", value: unknown): Promise<void>;
}

export interface ReviewState {
  scene: string;
  status: "busy" | "done" | "failed" | "consent";
  message: string;
}

export class AiState {
  settings = $state<AiSettings>(loadSettings());
  /** The latest review's findings for each scene, shown with the script check. */
  findings = $state<Record<string, AiFinding[]>>({});
  /** The review under way or just finished, for the bar above the script. */
  review = $state<ReviewState | null>(null);

  constructor(private app: Host) {}

  /** Change and remember some of the settings. */
  save(changes: Partial<AiSettings>) {
    const next = { ...this.settings, ...changes };
    next.url = next.url.trim() || DEFAULT_OLLAMA_URL;
    next.model = next.model.trim();
    next.claudeModel ||= DEFAULT_CLAUDE_MODEL;
    this.settings = next;
    for (const k of Object.keys(PREFS) as (keyof AiSettings)[]) writePref(PREFS[k], next[k]);
  }

  /** Forget the findings and the bar (a project closed or opened). */
  reset() {
    this.findings = {};
    this.review = null;
  }

  /** Let (or stop) the AI review sending this project's text to Anthropic. */
  async allowCloud(on: boolean) {
    await this.app.setOption("ai", on ? { cloud: true } : null);
  }

  /**
   * Review the open scene. Runs beside saving and the rule checks (never
   * instead of them); a failure only shows its reason.
   */
  async reviewScene(body: string) {
    const scene = this.app.scene?.id;
    const project = this.app.project;
    if (!scene || !project || this.review?.status === "busy") return;
    const { provider, url } = this.settings;
    const model = provider === "claude" ? this.settings.claudeModel : this.settings.model;
    const name = (m: string | null) => (provider === "claude" ? claudeName(m ?? model) : m || "the local model");
    this.review = {
      scene,
      status: "busy",
      message: `Reviewing with ${name(model)}…${provider === "ollama" ? " on a laptop this can take a few minutes." : ""}`,
    };
    const r = await ai.review({ root: project.root, scene, body, provider, url, model });
    if (this.review?.scene !== scene) return; // the project closed meanwhile
    if (r.ok) {
      this.findings = { ...this.findings, [scene]: r.findings };
      const n = r.findings.length;
      const found = n === 0 ? "nothing found" : n === 1 ? "1 note" : `${n} notes`;
      const cost = r.cost_usd != null ? ` Cost: ${dollars(r.cost_usd)}.` : "";
      this.review = { scene, status: "done", message: `${name(r.model)}: ${found}, marked “AI” in the script.${cost}` };
    } else {
      this.review = { scene, status: r.needs_consent ? "consent" : "failed", message: r.reason };
    }
    this.app.checksVersion++;
  }

  /** Stop showing the AI's findings for a scene. */
  clear(scene: string) {
    const { [scene]: _, ...rest } = this.findings;
    this.findings = rest;
    if (this.review?.scene === scene && this.review.status !== "busy") this.review = null;
    this.app.checksVersion++;
  }
}
