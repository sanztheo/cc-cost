//! Agrégation des records dédupliqués + état de l'interface.

use std::collections::BTreeMap;

use chrono::{Local, NaiveDate};

use crate::pricing::{self, Family};
use crate::scan::{Record, ScanResult};

/// Accumulateur de tokens + coût.
#[derive(Default, Clone)]
pub struct Agg {
    pub requests: u64,
    pub input: u64,
    pub output: u64,
    pub cache_5m: u64,
    pub cache_1h: u64,
    pub cache_read: u64,
    pub cost: f64,
    pub cost_no_cache: f64,
}

impl Agg {
    fn add(&mut self, r: &Record) {
        self.requests += 1;
        self.input += r.input;
        self.output += r.output;
        self.cache_5m += r.cache_5m;
        self.cache_1h += r.cache_1h;
        self.cache_read += r.cache_read;
        self.cost += r.cost;
        self.cost_no_cache +=
            pricing::cost_no_cache(r.family, r.input, r.output, r.cache_5m + r.cache_1h, r.cache_read);
    }

    pub fn cache_create(&self) -> u64 {
        self.cache_5m + self.cache_1h
    }

    pub fn tokens_total(&self) -> u64 {
        self.input + self.output + self.cache_create() + self.cache_read
    }

    /// Économie réelle apportée par le prompt-caching, en USD.
    pub fn savings(&self) -> f64 {
        (self.cost_no_cache - self.cost).max(0.0)
    }
}

pub struct ModelRow {
    pub model: String,
    pub family: Family,
    pub agg: Agg,
}

pub struct DayRow {
    pub day: NaiveDate,
    pub agg: Agg,
}

pub struct ProjectRow {
    pub project: String,
    pub agg: Agg,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Overview,
    Models,
    Timeline,
    Projects,
    Cache,
}

impl View {
    pub const ALL: [View; 5] = [
        View::Overview,
        View::Models,
        View::Timeline,
        View::Projects,
        View::Cache,
    ];

    pub fn index(self) -> usize {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }

    pub fn title(self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Models => "Modèles",
            View::Timeline => "Timeline",
            View::Projects => "Projets",
            View::Cache => "Cache",
        }
    }
}

pub struct App {
    pub total: Agg,
    pub by_model: Vec<ModelRow>,
    pub by_day: Vec<DayRow>,
    pub by_project: Vec<ProjectRow>,
    pub first_day: Option<NaiveDate>,
    pub last_day: Option<NaiveDate>,
    pub cost_today: f64,
    pub cost_7d: f64,
    pub cost_30d: f64,
    pub files: usize,
    pub dropped_dupes: usize,
    pub skipped: usize,
    pub unknown_models: Vec<String>,
    pub view: View,
    pub selected: usize,
}

impl App {
    pub fn build(scan: ScanResult) -> App {
        let mut total = Agg::default();
        let mut models: BTreeMap<String, (Family, Agg)> = BTreeMap::new();
        let mut days: BTreeMap<NaiveDate, Agg> = BTreeMap::new();
        let mut projects: BTreeMap<String, Agg> = BTreeMap::new();

        for r in &scan.records {
            total.add(r);
            models.entry(r.model.clone()).or_insert((r.family, Agg::default())).1.add(r);
            days.entry(r.day).or_default().add(r);
            projects.entry(r.project.clone()).or_default().add(r);
        }

        let mut by_model: Vec<ModelRow> = models
            .into_iter()
            .map(|(model, (family, agg))| ModelRow { model, family, agg })
            .collect();
        by_model.sort_by(|a, b| b.agg.cost.total_cmp(&a.agg.cost));

        let by_day: Vec<DayRow> = days.into_iter().map(|(day, agg)| DayRow { day, agg }).collect();

        let mut by_project: Vec<ProjectRow> = projects
            .into_iter()
            .map(|(project, agg)| ProjectRow { project, agg })
            .collect();
        by_project.sort_by(|a, b| b.agg.cost.total_cmp(&a.agg.cost));

        let first_day = by_day.first().map(|d| d.day);
        let last_day = by_day.last().map(|d| d.day);

        let today = Local::now().date_naive();
        let (mut cost_today, mut cost_7d, mut cost_30d) = (0.0, 0.0, 0.0);
        for d in &by_day {
            if d.day > today {
                continue;
            }
            let age = (today - d.day).num_days();
            if d.day == today {
                cost_today += d.agg.cost;
            }
            if age < 7 {
                cost_7d += d.agg.cost;
            }
            if age < 30 {
                cost_30d += d.agg.cost;
            }
        }

        App {
            total,
            by_model,
            by_day,
            by_project,
            first_day,
            last_day,
            cost_today,
            cost_7d,
            cost_30d,
            files: scan.files,
            dropped_dupes: scan.dropped_dupes,
            skipped: scan.skipped,
            unknown_models: scan.unknown_models,
            view: View::Overview,
            selected: 0,
        }
    }

    pub fn row_count(&self) -> usize {
        match self.view {
            View::Models => self.by_model.len(),
            View::Timeline => self.by_day.len(),
            View::Projects => self.by_project.len(),
            _ => 0,
        }
    }

    pub fn next_view(&mut self) {
        let i = (self.view.index() + 1) % View::ALL.len();
        self.view = View::ALL[i];
        self.selected = 0;
    }

    pub fn prev_view(&mut self) {
        let i = (self.view.index() + View::ALL.len() - 1) % View::ALL.len();
        self.view = View::ALL[i];
        self.selected = 0;
    }

    pub fn set_view_digit(&mut self, c: char) {
        if let Some(d) = c.to_digit(10) {
            if d >= 1 && (d as usize) <= View::ALL.len() {
                self.view = View::ALL[d as usize - 1];
                self.selected = 0;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        let n = self.row_count();
        if n > 0 {
            self.selected = (self.selected + 1).min(n - 1);
        }
    }

    pub fn scroll_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Rapport texte (mode `--report`), sert aussi de validation hors-TUI.
    pub fn print_report(&self) {
        println!("\n=== cc-cost — rapport ===");
        println!(
            "Période : {} → {}",
            self.first_day.map(|d| d.to_string()).unwrap_or_else(|| "?".into()),
            self.last_day.map(|d| d.to_string()).unwrap_or_else(|| "?".into())
        );
        println!(
            "Fichiers {} · messages facturés {} · doublons ignorés {} · synthetic/exclus {}",
            self.files,
            self.total.requests,
            self.dropped_dupes,
            self.skipped
        );
        println!("\nPar modèle :");
        println!(
            "  {:<32} {:>8} {:>12} {:>12}",
            "model", "msgs", "tokens", "coût $"
        );
        for m in &self.by_model {
            println!(
                "  {:<32} {:>8} {:>12} {:>12.2}",
                m.model,
                m.agg.requests,
                m.agg.tokens_total(),
                m.agg.cost
            );
        }
        println!("\nCOÛT TOTAL ESTIMÉ : ${:.2}", self.total.cost);
        println!(
            "Sans cache : ${:.2}  →  économie cache ${:.2} ({:.1}%)",
            self.total.cost_no_cache,
            self.total.savings(),
            if self.total.cost_no_cache > 0.0 {
                self.total.savings() / self.total.cost_no_cache * 100.0
            } else {
                0.0
            }
        );
        if !self.unknown_models.is_empty() {
            println!("\n⚠ Modèles inconnus (coût non calculé) : {:?}", self.unknown_models);
        }
    }
}
