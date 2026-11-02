mod relay;
mod serve;

use clap::{Parser, Subcommand};
use stagewright_core::Project;
use stagewright_core::analysis::{DoublingIssue, SceneStats, doubling_issues, scene_stats};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "stagewright", version, about = "Playwriting-first scene tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse a project and report running times, cast by scene, and doubling problems.
    Check {
        /// Project folder (contains project.yaml and acts/).
        dir: PathBuf,
    },
    /// Open the editor in a browser at http://127.0.0.1:<port>/ (localhost only).
    Serve {
        /// Project folder to open.
        dir: Option<PathBuf>,
        #[arg(long, default_value_t = 4870)]
        port: u16,
        /// Built editor files (app/dist). Omit when using the Vite dev server.
        #[arg(long = "static")]
        static_dir: Option<PathBuf>,
    },
    /// Run the co-writing relay (a y-websocket server). Writers connect to ws://HOST:PORT/<room>.
    Relay {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 4444)]
        port: u16,
        /// Folder where each room's shared document is saved.
        #[arg(long, default_value = "relay-data")]
        data: PathBuf,
    },
    /// Export the script: a paginated PDF in the project's format, or Fountain (by file extension).
    Export {
        /// Project folder.
        dir: PathBuf,
        /// Output file: .pdf or .fountain
        out: PathBuf,
        /// One episode of a series (default: the whole project).
        #[arg(long)]
        episode: Option<u32>,
    },
    /// Print each body line's element type as JSON (used to keep the editor in step with the parser).
    Classify { files: Vec<PathBuf> },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Check { dir } => match Project::load(&dir) {
            Ok(p) => check(&p),
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        Cmd::Serve { dir, port, static_dir } => match serve::run(dir, port, static_dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        Cmd::Relay { host, port, data } => match relay::run(host, port, data) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        Cmd::Classify { files } => classify(&files),
        Cmd::Export { dir, out, episode } => {
            let p = match Project::load(&dir) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            let bytes = match ext.as_str() {
                "pdf" => stagewright_core::export::pdf(&p, episode),
                "fountain" | "txt" => stagewright_core::export::fountain(&p, episode).into_bytes(),
                _ => {
                    eprintln!("error: the output file must end in .pdf or .fountain");
                    return ExitCode::from(2);
                }
            };
            match std::fs::write(&out, bytes) {
                Ok(()) => {
                    if ext == "pdf" {
                        eprintln!("{} pages", stagewright_core::export::page_count(&p, episode));
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {}: {e}", out.display());
                    ExitCode::from(2)
                }
            }
        }
    }
}

fn classify(files: &[PathBuf]) -> ExitCode {
    let mut out = serde_json::Map::new();
    for f in files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s.replace("\r\n", "\n"),
            Err(e) => {
                eprintln!("{}: {e}", f.display());
                return ExitCode::from(2);
            }
        };
        let kinds: Vec<&str> = match stagewright_core::Scene::parse(&src) {
            Ok(scene) => scene.body.iter().map(|e| e.kind()).collect(),
            Err(e) => {
                eprintln!("{}: {e}", f.display());
                return ExitCode::from(2);
            }
        };
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        out.insert(name, serde_json::json!(kinds));
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
    ExitCode::SUCCESS
}

fn label(s: &SceneStats, p: &Project, i: usize) -> String {
    let sf = &p.scenes[i];
    let base = match (sf.act, sf.scene.meta.scene) {
        (0, _) => "P".into(),
        (a, Some(n)) => format!("{a}.{n}"),
        _ => s.id.clone(),
    };
    match sf.episode {
        Some(e) => format!("E{e} {base}"),
        None => base,
    }
}

fn mins(secs: f64) -> String {
    format!("{:.0}", secs / 60.0)
}

fn check(p: &Project) -> ExitCode {
    let stats: Vec<SceneStats> = p.scenes.iter().map(|sf| scene_stats(&sf.scene, &p.meta)).collect();
    let labels: Vec<String> = stats.iter().enumerate().map(|(i, s)| label(s, p, i)).collect();

    println!("{} — {} scenes\n", p.meta.title, stats.len());

    // Running time
    if p.meta.format.is_screen() {
        println!("RUNNING TIME ({}: from page count)", p.meta.format.label());
    } else {
        println!(
            "RUNNING TIME ({} words/min, {}s per lyric line)",
            p.meta.running_time.words_per_minute, p.meta.running_time.seconds_per_lyric_line
        );
    }
    println!(
        "  {:<8} {:<24} {:>5} {:>6} {:>6}  songs",
        "scene", "title", "min", "pages", "words"
    );
    let mut act_totals: Vec<(Option<u32>, f64)> = Vec::new();
    for (i, s) in stats.iter().enumerate() {
        let manual = if s.manual_seconds.is_some() { "*" } else { " " };
        println!(
            "  {:<8} {:<24} {:>4}{} {:>6} {:>6}  {}",
            labels[i],
            truncate(&s.title, 24),
            mins(s.seconds()),
            manual,
            stagewright_core::layout::eighths_label(s.eighths),
            s.dialogue_words,
            s.songs.join(", ")
        );
        let act = p.scenes[i].scene.meta.act;
        match act_totals.last_mut() {
            Some((a, t)) if *a == act => *t += s.seconds(),
            _ => act_totals.push((act, s.seconds())),
        }
    }
    let total: f64 = stats.iter().map(SceneStats::seconds).sum();
    let parts: Vec<String> = act_totals
        .iter()
        .map(|(a, t)| match a {
            Some(0) => format!("Prologue {}", mins(*t)),
            Some(n) => format!("Act {n} {}", mins(*t)),
            None => format!("? {}", mins(*t)),
        })
        .collect();
    println!(
        "  {} · total {} min   (* = manual duration)\n",
        parts.join(" · "),
        mins(total)
    );

    // Cast by scene
    let mut roles: Vec<String> = p.characters.iter().map(|c| c.cue.clone()).collect();
    let extra: BTreeSet<String> = stats
        .iter()
        .flat_map(|s| s.present.iter().cloned())
        .filter(|r| !roles.contains(r))
        .collect();
    roles.extend(extra);
    roles.retain(|r| stats.iter().any(|s| s.present.contains(r)));
    println!("CAST BY SCENE (speeches; • = onstage, silent)");
    print!("  {:<12}", "");
    for l in &labels {
        print!("{l:>5}");
    }
    println!("  track");
    for r in &roles {
        print!("  {:<12}", truncate(r, 12));
        for s in &stats {
            let cell = match s.speeches.get(r) {
                Some(n) => n.to_string(),
                None if s.present.contains(r) => "•".into(),
                None => "".into(),
            };
            print!("{cell:>5}");
        }
        let track = p
            .character_by_cue(r)
            .and_then(|c| c.track.clone())
            .unwrap_or_else(|| "—".into());
        println!("  {track}");
    }
    println!();

    // Doubling
    let issues = doubling_issues(p, &stats);
    let label_of = |id: &str| {
        stats
            .iter()
            .position(|s| s.id == id)
            .map(|i| labels[i].clone())
            .unwrap_or_else(|| id.to_string())
    };
    println!("DOUBLING");
    let mut clashes = 0;
    for i in &issues {
        match i {
            DoublingIssue::Clash { scene, track, roles } => {
                clashes += 1;
                println!("  CLASH        {} {}: {}", label_of(scene), track, roles.join(" + "));
            }
            DoublingIssue::QuickChange {
                from_scene,
                to_scene,
                track,
                from_role,
                to_role,
            } => {
                println!(
                    "  quick change {}→{} {}: {} → {}",
                    label_of(from_scene),
                    label_of(to_scene),
                    track,
                    from_role,
                    to_role
                );
            }
            DoublingIssue::Unassigned { role, scenes } => {
                let s: Vec<String> = scenes.iter().map(|x| label_of(x)).collect();
                println!("  no track     {} ({})", role, s.join(", "));
            }
        }
    }
    if clashes == 0 {
        println!("  no clashes");
    }

    // Script check
    let mut ws = stagewright_api::Workspace::new(Some(p.root.clone()));
    if let Ok(r) = stagewright_api::dispatch(&mut ws, "check_project", serde_json::Value::Null) {
        let issues = r["issues"].as_array().cloned().unwrap_or_default();
        println!("\nSCRIPT CHECK ({} warnings, {} notes)", r["warnings"], r["infos"]);
        for i in &issues {
            println!(
                "  {:<8} {:<6} line {:<4} {:<22} {}",
                if i["severity"] == "warning" { "warning" } else { "note" },
                i["label"].as_str().unwrap_or(""),
                i["line"].as_u64().unwrap_or(0) + 1,
                i["code"].as_str().unwrap_or(""),
                i["message"].as_str().unwrap_or("")
            );
        }
        if issues.is_empty() {
            println!("  nothing to report");
        }
    }
    if clashes > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}
