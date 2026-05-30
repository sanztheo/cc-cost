//! Découverte des transcripts JSONL, parsing tolérant, déduplication.
//!
//! WHY la dédup est critique : un `claude -c` (resume) réécrit les mêmes lignes
//! assistant dans un nouveau fichier de session. Sur ce dataset, 58% des paires
//! `(message.id, requestId)` sont des doublons. Sans dédup, le coût est ~2.4×
//! la réalité. On garde la première occurrence de chaque clé.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Local, NaiveDate};
use rayon::prelude::*;
use serde::Deserialize;
use walkdir::WalkDir;

use crate::pricing::{self, Family};

/// Une ligne assistant valide, avant déduplication.
struct Parsed {
    key: String, // "message.id::requestId"
    model: String,
    day: NaiveDate, // jour LOCAL du message
    project: String,
    input: u64,
    output: u64,
    cache_5m: u64,
    cache_1h: u64,
    cache_read: u64,
}

/// Une ligne dédupliquée et tarifée, prête à agréger.
pub struct Record {
    pub model: String,
    pub family: Family,
    pub day: NaiveDate,
    pub project: String,
    pub input: u64,
    pub output: u64,
    pub cache_5m: u64,
    pub cache_1h: u64,
    pub cache_read: u64,
    pub cost: f64,
}

pub struct ScanResult {
    pub records: Vec<Record>,
    pub files: usize,
    pub dropped_dupes: usize,
    pub skipped: usize, // synthetic + modèles non facturables
    pub unknown_models: Vec<String>,
}

// --- structs serde (désérialisation tolérante : Option partout) ---

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    line_type: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    cwd: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

/// Détermine les racines à scanner. `CLAUDE_CONFIG_DIR` (CSV `:`) prioritaire,
/// sinon les emplacements par défaut. Les symlinks ne sont jamais suivis.
pub fn default_roots() -> Vec<PathBuf> {
    if let Ok(cfg) = std::env::var("CLAUDE_CONFIG_DIR") {
        return cfg
            .split(':')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let p = PathBuf::from(s);
                if p.ends_with("projects") {
                    p
                } else {
                    p.join("projects")
                }
            })
            .collect();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        PathBuf::from(&home).join(".claude/projects"),
        PathBuf::from(&home).join(".config/claude/projects"),
    ]
}

fn collect_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            // WHY skip symlink : un .jsonl symlinké dans projects pointe vers une
            // cible déjà scannée → double comptage.
            if entry.path_is_symlink() {
                continue;
            }
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|e| e == "jsonl")
            {
                files.push(entry.into_path());
            }
        }
    }
    files
}

fn parse_file(path: &Path) -> Vec<Parsed> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut out = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        // Pré-filtre bytes : la majorité des lignes (user/tool) n'ont pas d'usage.
        if !line.contains("\"usage\"") {
            continue;
        }
        if !line.contains("\"type\":\"assistant\"") && !line.contains("\"role\":\"assistant\"") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Line>(&line) else {
            continue;
        };
        if parsed.line_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(msg) = parsed.message else { continue };
        let Some(usage) = msg.usage else { continue };
        let id = msg.id.unwrap_or_default();
        if id.is_empty() {
            continue; // sans id pas de dédup fiable (0 cas réel sur le dataset)
        }
        let Some(ts) = parsed.timestamp.as_deref().and_then(parse_day) else {
            continue;
        };

        let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
        let (cache_5m, cache_1h) = match usage.cache_creation {
            Some(cc) => {
                let e5 = cc.ephemeral_5m_input_tokens.unwrap_or(0);
                let e1 = cc.ephemeral_1h_input_tokens.unwrap_or(0);
                if e5 + e1 > 0 {
                    (e5, e1)
                } else {
                    (cache_creation, 0) // fallback : tout en 5m (défaut Claude Code)
                }
            }
            None => (cache_creation, 0),
        };

        let request_id = parsed.request_id.unwrap_or_else(|| "none".to_string());
        out.push(Parsed {
            key: format!("{id}::{request_id}"),
            model: msg.model.unwrap_or_default(),
            day: ts,
            project: parsed.cwd.unwrap_or_else(|| "(inconnu)".to_string()),
            input: usage.input_tokens.unwrap_or(0),
            output: usage.output_tokens.unwrap_or(0),
            cache_5m,
            cache_1h,
            cache_read: usage.cache_read_input_tokens.unwrap_or(0),
        });
    }
    out
}

fn parse_day(ts: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.with_timezone(&Local).date_naive())
}

pub fn scan(roots: &[PathBuf]) -> Result<ScanResult> {
    let files = collect_files(roots);
    let file_count = files.len();

    // Parsing parallèle (rayon) : 2.6 Go de JSONL en quelques secondes.
    let parsed: Vec<Parsed> = files.par_iter().flat_map(|p| parse_file(p)).collect();
    let total_parsed = parsed.len();

    let mut seen: HashSet<String> = HashSet::with_capacity(total_parsed);
    let mut records = Vec::with_capacity(total_parsed);
    let mut unknown: HashMap<String, ()> = HashMap::new();
    let mut dropped_dupes = 0usize;
    let mut skipped = 0usize;

    for mut p in parsed {
        if !seen.insert(std::mem::take(&mut p.key)) {
            dropped_dupes += 1;
            continue;
        }
        let family = pricing::resolve_family(&p.model);
        if family == Family::Other {
            skipped += 1;
            // modèle inconnu (≠ synthetic) → on le signale, jamais compté en 0 muet.
            if !p.model.is_empty() && !p.model.to_ascii_lowercase().contains("synthetic") {
                unknown.insert(p.model.clone(), ());
            }
            continue;
        }
        let cost = pricing::cost_usd(family, p.input, p.output, p.cache_5m, p.cache_1h, p.cache_read);
        records.push(Record {
            model: p.model,
            family,
            day: p.day,
            project: p.project,
            input: p.input,
            output: p.output,
            cache_5m: p.cache_5m,
            cache_1h: p.cache_1h,
            cache_read: p.cache_read,
            cost,
        });
    }

    let mut unknown_models: Vec<String> = unknown.into_keys().collect();
    unknown_models.sort();

    Ok(ScanResult {
        records,
        files: file_count,
        dropped_dupes,
        skipped,
        unknown_models,
    })
}
