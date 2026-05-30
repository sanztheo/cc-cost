//! Interface TUI ratatui : onglets + 5 vues + event loop clavier.

use std::time::Duration;

use anyhow::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table, TableState, Tabs, Wrap},
    Frame,
};

use crate::app::{Agg, App, View};

// --- thème ---
const ACCENT: Color = Color::Cyan;
const MONEY: Color = Color::LightGreen;
const WARN: Color = Color::Yellow;
const DIM: Color = Color::DarkGray;
const HEAD: Color = Color::Magenta;

pub fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

/// Rend chaque vue une fois via un backend de test et l'imprime en texte.
/// WHY : vérifier que le rendu ne panique pas et inspecter le layout hors TTY.
pub fn snapshot(app: &mut App) -> Result<()> {
    // Vérif filtrage fenêtre : les totaux doivent croître Today ≤ … ≤ All.
    println!("=== totaux par fenêtre (doivent croître) ===");
    for w in crate::app::Window::ALL {
        app.set_window(w);
        println!("  {:<12} {:>12}  ({} jours actifs)", w.label(), fmt_usd(app.total.cost), app.active_days());
    }
    app.set_window(crate::app::Window::All);

    for v in View::ALL {
        app.view = v;
        app.selected = 0;
        dump(app, v.title())?;
    }

    // Timeline en granularité semaine (exerce l'autre chemin de rendu).
    app.view = View::Timeline;
    app.toggle_gran();
    dump(app, "Timeline (semaine)")?;
    Ok(())
}

fn dump(app: &mut App, title: &str) -> Result<()> {
    use ratatui::{backend::TestBackend, Terminal};
    let mut term = Terminal::new(TestBackend::new(118, 34))?;
    term.draw(|f| draw(f, app))?;
    let buf = term.backend().buffer().clone();
    println!("\n========== VUE : {title} ==========");
    let area = buf.area();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        println!("{}", line.trim_end());
    }
    Ok(())
}

fn run_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => app.next_view(),
                    KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => app.prev_view(),
                    KeyCode::Char(c @ '1'..='5') => app.set_view_digit(c),
                    KeyCode::Char(']') | KeyCode::Char('+') => app.cycle_window(1),
                    KeyCode::Char('[') | KeyCode::Char('-') => app.cycle_window(-1),
                    KeyCode::Char('w') => app.toggle_gran(),
                    KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());
    draw_tabs(f, chunks[0], app);
    match app.view {
        View::Overview => draw_overview(f, chunks[1], app),
        View::Models => draw_models(f, chunks[1], app),
        View::Timeline => draw_timeline(f, chunks[1], app),
        View::Projects => draw_projects(f, chunks[1], app),
        View::Cache => draw_cache(f, chunks[1], app),
    }
    draw_footer(f, chunks[2], app);
}

fn draw_tabs(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = View::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| Line::from(format!(" {} {} ", i + 1, v.title())))
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.view.index())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Span::styled(
                    " cc-cost · estimateur coût API Claude Code ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ))
                .title_top(
                    ratatui::text::Line::from(Span::styled(
                        format!(" période : {} ", app.window.label()),
                        Style::default().fg(MONEY).add_modifier(Modifier::BOLD),
                    ))
                    .right_aligned(),
                ),
        )
        .highlight_style(Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
        .divider(Span::styled("│", Style::default().fg(DIM)));
    f.render_widget(tabs, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let key = |s: &'static str| Span::styled(s, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    let dim = |s: String| Span::styled(s, Style::default().fg(DIM));
    let spans = vec![
        key(" q"),
        dim(" quitter  ".into()),
        key("Tab/←→/1-5"),
        dim(" vues  ".into()),
        key("[ ]"),
        dim(format!(" période:{}  ", app.window.label())),
        key("w"),
        dim(format!(" gran:{}  ", app.gran.label())),
        key("j/k"),
        dim(" défiler".into()),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(HEAD).add_modifier(Modifier::BOLD),
        ))
}

// --- Vue 1 : Overview ---

fn draw_overview(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.total;
    let all = &app.all_time;
    let all_days = match (app.all_first_day, app.all_last_day) {
        (Some(a), Some(b)) => (b - a).num_days() + 1,
        _ => 0,
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("  COÛT ({})   ", app.window.label()), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(fmt_usd(t.cost), Style::default().fg(MONEY).add_modifier(Modifier::BOLD)),
        Span::styled(format!("     ·  tout temps : {}", fmt_usd(all.cost)), Style::default().fg(DIM)),
    ]));
    lines.push(Line::from(""));
    lines.push(kv("Données complètes", format!(
        "{} → {}  ({} jours) · {} msgs · {} fichiers",
        app.all_first_day.map(|d| d.to_string()).unwrap_or_default(),
        app.all_last_day.map(|d| d.to_string()).unwrap_or_default(),
        all_days,
        fmt_int(all.requests),
        fmt_int(app.files as u64),
    )));
    lines.push(kv("Dédup / exclus", format!(
        "{} doublons ignorés · {} synthetic exclus",
        fmt_int(app.dropped_dupes as u64),
        fmt_int(app.skipped as u64),
    )));

    lines.push(Line::from(Span::styled("  ──────── rythme (fenêtre courante) ────────", Style::default().fg(DIM))));
    lines.push(kv("Messages facturés", fmt_int(t.requests)));
    lines.push(kv("Jours actifs", fmt_int(app.active_days() as u64)));
    lines.push(kv("Moyenne/jour actif", fmt_usd(app.avg_per_active_day())));
    if let Some(p) = app.peak_day() {
        lines.push(kv("Jour le plus cher", format!("{} — {}", p.day, fmt_usd(p.agg.cost))));
    }
    lines.push(kv("Projection 30 j", format!("{} (au rythme moyen actuel)", fmt_usd(app.run_rate_30d()))));

    lines.push(Line::from(Span::styled("  ──────── tokens (fenêtre) ────────", Style::default().fg(DIM))));
    lines.push(kv("Input / Output", format!("{} / {}", fmt_tokens(t.input), fmt_tokens(t.output))));
    lines.push(kv("Cache write", format!("{} (5m {} · 1h {})", fmt_tokens(t.cache_create()), fmt_tokens(t.cache_5m), fmt_tokens(t.cache_1h))));
    lines.push(kv("Cache read", fmt_tokens(t.cache_read)));
    lines.push(kv("Total tokens", fmt_tokens(t.tokens_total())));

    lines.push(Line::from(Span::styled("  ──────── repères (tout temps) ────────", Style::default().fg(DIM))));
    lines.push(Line::from(vec![
        Span::styled("  Aujourd'hui ", Style::default().fg(DIM)),
        Span::styled(fmt_usd(app.cost_today), Style::default().fg(MONEY)),
        Span::styled("   7 j ", Style::default().fg(DIM)),
        Span::styled(fmt_usd(app.cost_7d), Style::default().fg(MONEY)),
        Span::styled("   30 j ", Style::default().fg(DIM)),
        Span::styled(fmt_usd(app.cost_30d), Style::default().fg(MONEY)),
    ]));
    lines.push(kv("Économie cache", format!(
        "{}  ({:.0}% vs sans cache)",
        fmt_usd(t.savings()),
        if t.cost_no_cache > 0.0 { t.savings() / t.cost_no_cache * 100.0 } else { 0.0 }
    )));
    if let Some(m) = app.by_model.first() {
        let proj = app
            .by_project
            .first()
            .map(|p| format!("   ·   projet n°1 : {} {}", short_path(&p.project), fmt_usd(p.agg.cost)))
            .unwrap_or_default();
        lines.push(kv("Modèle n°1", format!("{} — {}{}", m.model, fmt_usd(m.agg.cost), proj)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ⚠ Estimation au tarif API à la demande — PAS ta facture réelle (plan Max/Pro inclut l'usage).",
        Style::default().fg(WARN),
    )));
    if !app.unknown_models.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ Modèles inconnus non chiffrés : {}", app.unknown_models.join(", ")),
            Style::default().fg(WARN),
        )));
    }

    let p = Paragraph::new(lines)
        .block(block(&format!("Vue d'ensemble — {}", app.window.label())))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn kv(key: &str, val: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<22}"), Style::default().fg(DIM)),
        Span::styled(val, Style::default().fg(Color::White)),
    ])
}

// --- Vue 2 : Modèles ---

fn draw_models(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["Modèle", "Famille", "Msgs", "Input", "Output", "CacheW", "CacheR", "Coût", "%"])
        .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    let total = app.total.cost.max(f64::MIN_POSITIVE);
    let rows: Vec<Row> = app
        .by_model
        .iter()
        .map(|m| {
            let pct = m.agg.cost / total * 100.0;
            Row::new(vec![
                Cell::from(m.model.clone()),
                Cell::from(m.family.label()),
                Cell::from(fmt_int(m.agg.requests)),
                Cell::from(fmt_tokens(m.agg.input)),
                Cell::from(fmt_tokens(m.agg.output)),
                Cell::from(fmt_tokens(m.agg.cache_create())),
                Cell::from(fmt_tokens(m.agg.cache_read)),
                Cell::from(Span::styled(fmt_usd(m.agg.cost), Style::default().fg(MONEY))),
                Cell::from(format!("{pct:.1}")),
            ])
        })
        .collect();
    let widths = [
        Constraint::Min(22),
        Constraint::Length(18),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(11),
        Constraint::Length(6),
    ];
    render_table(f, area, "Coût par modèle", header, rows, &widths, app.selected);
}

// --- Vue 3 : Timeline ---

fn draw_timeline(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).split(area);

    let buckets = app.timeline_buckets(); // du plus ancien au plus récent
    let data: Vec<u64> = buckets.iter().map(|(_, a)| (a.cost * 100.0) as u64).collect();
    let max_bucket = buckets.iter().map(|(_, a)| a.cost).fold(0.0_f64, f64::max);
    let spark = Sparkline::default()
        .block(block(&format!(
            "Coût / {} — fenêtre {} · moy/jour actif {} · pic {}",
            app.gran.label(),
            app.window.label(),
            fmt_usd(app.avg_per_active_day()),
            fmt_usd(max_bucket),
        )))
        .data(&data)
        .style(Style::default().fg(ACCENT));
    f.render_widget(spark, chunks[0]);

    // Cumul calculé en ordre chronologique, affiché récent en haut.
    let mut running = 0.0;
    let mut rows_data: Vec<(String, Agg, f64)> = Vec::with_capacity(buckets.len());
    for (label, agg) in buckets.iter() {
        running += agg.cost;
        rows_data.push((label.clone(), agg.clone(), running));
    }
    let max = max_bucket.max(f64::MIN_POSITIVE);

    let header = Row::new(vec!["Période", "Msgs", "Coût", "Cumul", "Tendance"])
        .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = rows_data
        .iter()
        .rev()
        .map(|(label, agg, cumul)| {
            Row::new(vec![
                Cell::from(label.clone()),
                Cell::from(fmt_int(agg.requests)),
                Cell::from(Span::styled(fmt_usd(agg.cost), Style::default().fg(MONEY))),
                Cell::from(Span::styled(fmt_usd(*cumul), Style::default().fg(DIM))),
                Cell::from(Span::styled(bar(agg.cost / max, 24), Style::default().fg(ACCENT))),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(16),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Min(20),
    ];
    render_table(f, chunks[1], &format!("Détail par {} (récent en haut)", app.gran.label()), header, rows, &widths, app.selected);
}

// --- Vue 4 : Projets ---

fn draw_projects(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["Projet", "Msgs", "Coût", "%", "Part"])
        .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    let total = app.total.cost.max(f64::MIN_POSITIVE);
    let max = app.by_project.iter().map(|p| p.agg.cost).fold(0.0_f64, f64::max).max(f64::MIN_POSITIVE);
    let rows: Vec<Row> = app
        .by_project
        .iter()
        .map(|p| {
            let pct = p.agg.cost / total * 100.0;
            Row::new(vec![
                Cell::from(short_path(&p.project)),
                Cell::from(fmt_int(p.agg.requests)),
                Cell::from(Span::styled(fmt_usd(p.agg.cost), Style::default().fg(MONEY))),
                Cell::from(format!("{pct:.1}")),
                Cell::from(Span::styled(bar(p.agg.cost / max, 24), Style::default().fg(ACCENT))),
            ])
        })
        .collect();
    let widths = [
        Constraint::Min(30),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(6),
        Constraint::Length(26),
    ];
    render_table(f, area, "Coût par projet", header, rows, &widths, app.selected);
}

// --- Vue 5 : Cache savings ---

fn draw_cache(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
    let t = &app.total;

    // % des tokens d'input servis par le cache (read) vs total pool d'input.
    let input_pool = (t.input + t.cache_create() + t.cache_read).max(1);
    let cached_ratio = t.cache_read as f64 / input_pool as f64;
    let gauge = Gauge::default()
        .block(block("Part des tokens d'input servis par le cache"))
        .gauge_style(Style::default().fg(MONEY).bg(DIM))
        .ratio(cached_ratio.clamp(0.0, 1.0))
        .label(format!("{:.1}%", cached_ratio * 100.0));
    f.render_widget(gauge, chunks[0]);

    let savings_pct = if t.cost_no_cache > 0.0 { t.savings() / t.cost_no_cache * 100.0 } else { 0.0 };
    let cache_read_cost = cache_read_cost(app);
    let lines = vec![
        kv("Cache read tokens", format!("{}  (coût ≈ {})", fmt_tokens(t.cache_read), fmt_usd(cache_read_cost))),
        kv("Cache write tokens", format!("{} (5m {} · 1h {})", fmt_tokens(t.cache_create()), fmt_tokens(t.cache_5m), fmt_tokens(t.cache_1h))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Coût réel (avec cache)        ", Style::default().fg(DIM)),
            Span::styled(fmt_usd(t.cost), Style::default().fg(MONEY).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Coût hypothétique sans cache  ", Style::default().fg(DIM)),
            Span::styled(fmt_usd(t.cost_no_cache), Style::default().fg(WARN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ÉCONOMIE GRÂCE AU CACHE       ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ({savings_pct:.0}%)", fmt_usd(t.savings())), Style::default().fg(MONEY).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Le cache read est facturé 0.1× le tarif input → relire le contexte coûte 10× moins que le renvoyer.",
            Style::default().fg(DIM),
        )),
    ];
    f.render_widget(Paragraph::new(lines).block(block("Économie du prompt-caching")).wrap(Wrap { trim: false }), chunks[1]);
}

/// Coût attribuable aux seuls cache_read tokens (somme par famille).
fn cache_read_cost(app: &App) -> f64 {
    app.by_model
        .iter()
        .map(|m| crate::pricing::cost_usd(m.family, 0, 0, 0, 0, m.agg.cache_read))
        .sum()
}

// --- helpers rendu ---

fn render_table(
    f: &mut Frame,
    area: Rect,
    title: &str,
    header: Row,
    rows: Vec<Row>,
    widths: &[Constraint],
    selected: usize,
) {
    let len = rows.len();
    let table = Table::new(rows, widths.to_vec())
        .header(header)
        .block(block(title))
        .row_highlight_style(Style::default().bg(Color::Rgb(40, 40, 60)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    let mut state = TableState::default();
    if len > 0 {
        state.select(Some(selected.min(len - 1)));
    }
    f.render_stateful_widget(table, area, &mut state);
}

fn bar(frac: f64, width: usize) -> String {
    let frac = frac.clamp(0.0, 1.0);
    let filled = (frac * width as f64).round() as usize;
    let mut s = "█".repeat(filled);
    s.push_str(&"░".repeat(width.saturating_sub(filled)));
    s
}

fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.trim_end_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let n = parts.len();
    if n <= 2 {
        p.to_string()
    } else {
        format!(".../{}/{}", parts[n - 2], parts[n - 1])
    }
}

fn fmt_usd(v: f64) -> String {
    let neg = v < 0.0;
    let s = format!("{:.2}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    format!("{}${}.{}", if neg { "-" } else { "" }, group_thousands(int), frac)
}

fn fmt_int(v: u64) -> String {
    group_thousands(&v.to_string())
}

fn fmt_tokens(v: u64) -> String {
    let f = v as f64;
    if f >= 1e9 {
        format!("{:.2}B", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.1}M", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.1}K", f / 1e3)
    } else {
        v.to_string()
    }
}

fn group_thousands(int: &str) -> String {
    let bytes = int.as_bytes();
    let mut out = String::with_capacity(int.len() + int.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(*b as char);
    }
    out
}
