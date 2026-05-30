//! Agrégation des records dédupliqués + état de l'interface.
//!
//! Les records bruts sont conservés pour pouvoir recalculer les agrégats à la
//! volée quand l'utilisateur change de fenêtre temporelle (Aujourd'hui … Tout)
//! ou de granularité (jour / semaine).

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, Local, NaiveDate};

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

    fn merge(&mut self, other: &Agg) {
        self.requests += other.requests;
        self.input += other.input;
        self.output += other.output;
        self.cache_5m += other.cache_5m;
        self.cache_1h += other.cache_1h;
        self.cache_read += other.cache_read;
        self.cost += other.cost;
        self.cost_no_cache += other.cost_no_cache;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Overview,
    Models,
    Timeline,
    Projects,
    Cache,
}

impl View {
    pub const ALL: [View; 5] = [View::Overview, View::Models, View::Timeline, View::Projects, View::Cache];

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

/// Fenêtre temporelle appliquée à toutes les vues, ancrée sur aujourd'hui.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    Today,
    D7,
    D14,
    D30,
    D90,
    All,
}

impl Window {
    pub const ALL: [Window; 6] = [Window::Today, Window::D7, Window::D14, Window::D30, Window::D90, Window::All];

    pub fn label(self) -> &'static str {
        match self {
            Window::Today => "Aujourd'hui",
            Window::D7 => "7 jours",
            Window::D14 => "14 jours",
            Window::D30 => "30 jours",
            Window::D90 => "90 jours",
            Window::All => "Tout",
        }
    }

    /// Nombre de jours couverts (None = illimité).
    pub fn days(self) -> Option<i64> {
        match self {
            Window::Today => Some(1),
            Window::D7 => Some(7),
            Window::D14 => Some(14),
            Window::D30 => Some(30),
            Window::D90 => Some(90),
            Window::All => None,
        }
    }

    fn index(self) -> usize {
        Window::ALL.iter().position(|w| *w == self).unwrap_or(0)
    }
}

/// Granularité de la timeline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gran {
    Daily,
    Weekly,
}

impl Gran {
    pub fn label(self) -> &'static str {
        match self {
            Gran::Daily => "jour",
            Gran::Weekly => "semaine",
        }
    }
}

pub struct App {
    // données brutes conservées pour recalcul à la volée
    records: Vec<Record>,
    anchor: NaiveDate, // aujourd'hui (ancrage des fenêtres)

    pub window: Window,
    pub gran: Gran,

    // agrégats recalculés pour la fenêtre courante
    pub total: Agg,
    pub by_model: Vec<ModelRow>,
    pub by_day: Vec<DayRow>,
    pub by_project: Vec<ProjectRow>,

    // contexte all-time (fixe)
    pub all_time: Agg,
    pub all_first_day: Option<NaiveDate>,
    pub all_last_day: Option<NaiveDate>,
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
        let anchor = Local::now().date_naive();

        // contexte all-time calculé une fois
        let mut all_time = Agg::default();
        let mut all_days: BTreeMap<NaiveDate, Agg> = BTreeMap::new();
        for r in &scan.records {
            all_time.add(r);
            all_days.entry(r.day).or_default().add(r);
        }
        let all_first_day = all_days.keys().next().copied();
        let all_last_day = all_days.keys().next_back().copied();

        let (mut cost_today, mut cost_7d, mut cost_30d) = (0.0, 0.0, 0.0);
        for (day, agg) in &all_days {
            if *day > anchor {
                continue;
            }
            let age = (anchor - *day).num_days();
            if *day == anchor {
                cost_today += agg.cost;
            }
            if age < 7 {
                cost_7d += agg.cost;
            }
            if age < 30 {
                cost_30d += agg.cost;
            }
        }

        let mut app = App {
            records: scan.records,
            anchor,
            window: Window::All,
            gran: Gran::Daily,
            total: Agg::default(),
            by_model: Vec::new(),
            by_day: Vec::new(),
            by_project: Vec::new(),
            all_time,
            all_first_day,
            all_last_day,
            cost_today,
            cost_7d,
            cost_30d,
            files: scan.files,
            dropped_dupes: scan.dropped_dupes,
            skipped: scan.skipped,
            unknown_models: scan.unknown_models,
            view: View::Overview,
            selected: 0,
        };
        app.recompute();
        app
    }

    /// Recalcule total/by_model/by_day/by_project pour la fenêtre courante.
    fn recompute(&mut self) {
        let cutoff = self.window.days().map(|d| self.anchor - Duration::days(d - 1));
        let mut total = Agg::default();
        let mut models: BTreeMap<String, (Family, Agg)> = BTreeMap::new();
        let mut days: BTreeMap<NaiveDate, Agg> = BTreeMap::new();
        let mut projects: BTreeMap<String, Agg> = BTreeMap::new();

        for r in &self.records {
            if let Some(c) = cutoff {
                if r.day < c || r.day > self.anchor {
                    continue;
                }
            }
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

        self.total = total;
        self.by_model = by_model;
        self.by_day = by_day;
        self.by_project = by_project;
        self.selected = 0;
    }

    // --- stats dérivées sur la fenêtre courante ---

    pub fn active_days(&self) -> usize {
        self.by_day.len()
    }

    pub fn avg_per_active_day(&self) -> f64 {
        if self.by_day.is_empty() {
            0.0
        } else {
            self.total.cost / self.by_day.len() as f64
        }
    }

    /// Projection mensuelle au rythme moyen actuel.
    pub fn run_rate_30d(&self) -> f64 {
        self.avg_per_active_day() * 30.0
    }

    pub fn peak_day(&self) -> Option<&DayRow> {
        self.by_day.iter().max_by(|a, b| a.agg.cost.total_cmp(&b.agg.cost))
    }

    /// Buckets de la timeline (jour ou semaine), du plus ancien au plus récent.
    pub fn timeline_buckets(&self) -> Vec<(String, Agg)> {
        match self.gran {
            Gran::Daily => self.by_day.iter().map(|d| (d.day.to_string(), d.agg.clone())).collect(),
            Gran::Weekly => {
                let mut weeks: BTreeMap<NaiveDate, Agg> = BTreeMap::new();
                for d in &self.by_day {
                    let monday = d.day - Duration::days(d.day.weekday().num_days_from_monday() as i64);
                    weeks.entry(monday).or_default().merge(&d.agg);
                }
                weeks.into_iter().map(|(m, a)| (format!("sem. {m}"), a)).collect()
            }
        }
    }

    // --- état UI ---

    pub fn row_count(&self) -> usize {
        match self.view {
            View::Models => self.by_model.len(),
            View::Timeline => self.timeline_buckets().len(),
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

    pub fn set_window(&mut self, w: Window) {
        self.window = w;
        self.recompute();
    }

    /// Change de fenêtre : dir +1 = plus large, -1 = plus courte.
    pub fn cycle_window(&mut self, dir: i32) {
        let n = Window::ALL.len() as i32;
        let i = (self.window.index() as i32 + dir).rem_euclid(n);
        self.window = Window::ALL[i as usize];
        self.recompute();
    }

    pub fn toggle_gran(&mut self) {
        self.gran = match self.gran {
            Gran::Daily => Gran::Weekly,
            Gran::Weekly => Gran::Daily,
        };
        self.selected = 0;
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

    /// Rapport texte (mode `--report`) — toujours sur l'ensemble des données.
    pub fn print_report(&self) {
        println!("\n=== cc-cost — rapport ===");
        println!(
            "Période : {} → {}",
            self.all_first_day.map(|d| d.to_string()).unwrap_or_else(|| "?".into()),
            self.all_last_day.map(|d| d.to_string()).unwrap_or_else(|| "?".into())
        );
        println!(
            "Fichiers {} · messages facturés {} · doublons ignorés {} · synthetic/exclus {}",
            self.files, self.all_time.requests, self.dropped_dupes, self.skipped
        );
        println!("\nPar modèle :");
        println!("  {:<32} {:>8} {:>12} {:>12}", "model", "msgs", "tokens", "coût $");
        for m in &self.by_model {
            println!(
                "  {:<32} {:>8} {:>12} {:>12.2}",
                m.model,
                m.agg.requests,
                m.agg.tokens_total(),
                m.agg.cost
            );
        }
        println!("\nCOÛT TOTAL ESTIMÉ : ${:.2}", self.all_time.cost);
        println!(
            "Sans cache : ${:.2}  →  économie cache ${:.2} ({:.1}%)",
            self.all_time.cost_no_cache,
            self.all_time.savings(),
            if self.all_time.cost_no_cache > 0.0 {
                self.all_time.savings() / self.all_time.cost_no_cache * 100.0
            } else {
                0.0
            }
        );
        if !self.unknown_models.is_empty() {
            println!("\n⚠ Modèles inconnus (coût non calculé) : {:?}", self.unknown_models);
        }
    }
}
