// Self-update for the desktop app (Tauri's updater plugin).
//
// The app checks a small file, latest.json, published with each release on
// GitHub. If it names a newer version, the app offers it; installing downloads
// the installer, checks its signature against the public key built into the
// app, saves everything, then installs and restarts. In a browser (`stagewright
// serve`) there is nothing to update, so this stays idle.

import { isTauri } from "./api";

declare const __APP_VERSION__: string;
export const APP_VERSION: string = typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "dev";

type Phase = "idle" | "checking" | "current" | "available" | "downloading" | "installing" | "error";

interface Pending {
  version: string;
  notes: string;
  date: string | null;
  downloadAndInstall: (onEvent: (e: DownloadEvent) => void) => Promise<void>;
}

type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

const RECHECK_MS = 6 * 60 * 60 * 1000;

class Updates {
  phase = $state<Phase>("idle");
  /** The newer version on offer, if any. */
  available = $state<{ version: string; notes: string; date: string | null } | null>(null);
  /** 0–1 while downloading; null when the size is unknown. */
  progress = $state<number | null>(null);
  error = $state<string | null>(null);
  /** The offer was put aside for this session ("Later"). */
  dismissed = $state(false);
  /** A check the person asked for, so "up to date" is worth saying. */
  manual = $state(false);

  readonly supported = isTauri();
  private pending: Pending | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;

  /** Check now, then every few hours while the app stays open. */
  start() {
    if (!this.supported || this.timer) return;
    void this.check();
    this.timer = setInterval(() => void this.check(), RECHECK_MS);
  }

  async check(manual = false) {
    if (!this.supported || this.phase === "checking" || this.phase === "downloading" || this.phase === "installing") return;
    this.manual = manual;
    this.phase = "checking";
    this.error = null;
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const u = await check();
      if (u) {
        this.pending = u as unknown as Pending;
        this.available = { version: u.version, notes: u.body ?? "", date: u.date ?? null };
        this.phase = "available";
        if (manual) this.dismissed = false;
      } else {
        this.pending = null;
        this.available = null;
        this.phase = "current";
      }
    } catch (e) {
      // Offline or GitHub unreachable: only worth mentioning when asked.
      this.phase = "error";
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  /** Save, download, verify, install and restart. `beforeInstall` flushes unsaved work. */
  async install(beforeInstall: () => Promise<void>) {
    if (!this.pending) return;
    this.phase = "downloading";
    this.progress = null;
    this.error = null;
    let total = 0;
    let got = 0;
    try {
      await beforeInstall();
      await this.pending.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          got += e.data.chunkLength;
          this.progress = total ? Math.min(1, got / total) : null;
        } else if (e.event === "Finished") this.phase = "installing";
      });
      // Windows has already handed over to the installer and closed by now;
      // on macOS the new app is in place and needs a restart.
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      this.phase = "error";
      this.error = e instanceof Error ? e.message : String(e);
    }
  }
}

export const updates = new Updates();

/** "- one\n- two" release notes as plain list items; other lines as paragraphs. */
export function noteItems(notes: string): string[] {
  const out: string[] = [];
  for (const raw of notes.split("\n")) {
    const line = raw.trimEnd();
    if (/^\s*[-*] /.test(line)) out.push(line.replace(/^\s*[-*] /, ""));
    else if (line.trim() && out.length && /^\s+/.test(raw)) out[out.length - 1] += " " + line.trim();
    else if (line.trim()) out.push(line.trim());
  }
  return out;
}
