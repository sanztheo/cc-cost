//! Tarifs API Anthropic officiels, en USD par million de tokens.
//! Vérifiés sur docs.claude.com/pricing le 2026-05-30.
//!
//! WHY pas de tiering long-context : aucun modèle présent dans les transcripts
//! (Opus 4.6/4.7/4.8, Sonnet 4.6, Haiku 4.5) n'a de palier premium au-delà de
//! 200k tokens — la fenêtre 1M est facturée au tarif standard. Inutile de coder
//! un mécanisme mort.

/// Famille de tarification. `Other` = synthetic ou modèle inconnu → coût 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Opus,
    Sonnet,
    Haiku,
    OpusLegacy,
    Other,
}

const TOKENS_PER_MILLION: f64 = 1_000_000.0;

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::Opus => "Opus 4.6/4.7/4.8",
            Family::Sonnet => "Sonnet 4.6",
            Family::Haiku => "Haiku 4.5",
            Family::OpusLegacy => "Opus 4/4.1 legacy",
            Family::Other => "autre / synthetic",
        }
    }

    /// Prix [input, output, cache_write_5m, cache_write_1h, cache_read] en $/Mtok.
    /// `None` pour `Other` (non facturable).
    fn rates_per_mtok(self) -> Option<[f64; 5]> {
        Some(match self {
            Family::Opus => [5.0, 25.0, 6.25, 10.0, 0.5],
            Family::Sonnet => [3.0, 15.0, 3.75, 6.0, 0.3],
            Family::Haiku => [1.0, 5.0, 1.25, 2.0, 0.1],
            Family::OpusLegacy => [15.0, 75.0, 18.75, 30.0, 1.5],
            Family::Other => return None,
        })
    }
}

/// Résout un model id brut vers sa famille de prix.
///
/// Tolère préfixe Bedrock `anthropic.`, suffixe date `-YYYYMMDD`, alias courts.
/// L'ordre compte : Opus legacy (4/4.1, 3× plus cher) testé avant Opus standard.
pub fn resolve_family(model: &str) -> Family {
    let lower = model.trim().to_ascii_lowercase();
    if lower.is_empty() || lower.contains("synthetic") {
        return Family::Other;
    }
    let s: &str = lower.strip_prefix("anthropic.").unwrap_or(&lower);

    if s.contains("opus") {
        // Opus 4 nu et 4.1 sont au tarif legacy $15/$75.
        if s.contains("opus-4-1") || s.contains("opus-4-2025") || s == "claude-opus-4" {
            return Family::OpusLegacy;
        }
        return Family::Opus;
    }
    if s.contains("sonnet") {
        return Family::Sonnet;
    }
    if s.contains("haiku") {
        return Family::Haiku;
    }
    Family::Other
}

/// Coût réel (avec cache) d'un message, en USD.
pub fn cost_usd(fam: Family, input: u64, output: u64, cache_5m: u64, cache_1h: u64, cache_read: u64) -> f64 {
    let Some(r) = fam.rates_per_mtok() else {
        return 0.0;
    };
    (input as f64 * r[0]
        + output as f64 * r[1]
        + cache_5m as f64 * r[2]
        + cache_1h as f64 * r[3]
        + cache_read as f64 * r[4])
        / TOKENS_PER_MILLION
}

/// Coût hypothétique SANS prompt-caching : tout ce qui fut mis en cache
/// (création + lecture) aurait été renvoyé en input frais plein tarif.
/// Sert à chiffrer l'économie réelle du cache.
pub fn cost_no_cache(fam: Family, input: u64, output: u64, cache_create: u64, cache_read: u64) -> f64 {
    let Some(r) = fam.rates_per_mtok() else {
        return 0.0;
    };
    ((input + cache_create + cache_read) as f64 * r[0] + output as f64 * r[1]) / TOKENS_PER_MILLION
}
