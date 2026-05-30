//! cc-cost — TUI d'analyse du coût API des conversations Claude Code.
//!
//! Scanne ~/.claude/projects, déduplique les messages (resume réécrit les
//! lignes), recalcule le coût au tarif API officiel, l'affiche dans un TUI.

mod app;
mod pricing;
mod scan;
mod ui;

use std::time::Instant;

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let report_only = args.iter().any(|a| a == "--report");
    let snapshot = args.iter().any(|a| a == "--snapshot");

    let roots = scan::default_roots();
    eprintln!("Scan : {}", roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", "));

    let started = Instant::now();
    let result = scan::scan(&roots)?;
    let total_cost: f64 = result.records.iter().map(|r| r.cost).sum();
    eprintln!(
        "{} fichiers · {} messages dédupliqués · {} doublons ignorés · {:.1}s · total ≈ ${:.2}",
        result.files,
        result.records.len(),
        result.dropped_dupes,
        started.elapsed().as_secs_f64(),
        total_cost,
    );

    if result.records.is_empty() {
        eprintln!("Aucun transcript trouvé. Définis CLAUDE_CONFIG_DIR ou vérifie ~/.claude/projects.");
        return Ok(());
    }

    let mut app = app::App::build(result);

    if report_only {
        app.print_report();
        return Ok(());
    }
    if snapshot {
        return ui::snapshot(&mut app);
    }

    ui::run(app)
}
