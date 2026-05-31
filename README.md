# cc-cost

**What would your Claude Code sessions have cost at pay-as-you-go API rates?**

`cc-cost` is a local, read-only Rust TUI that scans every Claude Code conversation on your machine and re-prices it from the real token counts — input, output, cache writes, and cache reads — against the official Anthropic API tariff.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](#license)
[![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-000000.svg?logo=rust)](https://www.rust-lang.org/)
[![Built with ratatui](https://img.shields.io/badge/Built%20with-ratatui-7c3aed.svg)](https://ratatui.rs/)

---

## Why

A Claude Max or Pro subscription is a flat fee. That's great for budgeting, but it completely hides what your actual usage is worth: a heavy day of agentic coding and a quiet day of one-off questions look identical on your invoice.

`cc-cost` answers a single, concrete question:

> If I had paid for the API one token at a time, what would these sessions have cost me?

It reads the transcripts Claude Code already writes to disk, recomputes the cost from the raw token usage, and presents the result across five focused views. No network, no account login, no writes — just your local data, priced.

## Features

Five TUI views, switchable at any time:

- **Overview** — total estimated cost, the period covered, total tokens, rolling windows (today / last 7 days / last 30 days), and how much the prompt cache saved you.
- **Models** — cost broken down per model, sorted, with each model's share of the total as a percentage.
- **Timeline** — a cost-per-day (or per-week) sparkline plus a table with running cumulative spend, so you can see spikes and trends over time.
- **Projects** — cost grouped by project folder, to see which codebases are the expensive ones.
- **Cache** — what percentage of input was served from cache, and the dollar amount the cache saved versus paying full input price.

> Sample output (fictional data):

```text
╭─ cc-cost ──────────────────────────────────────────────────────────────────────────────────────╮
│  [1]Overview   2 Modeles   3 Timeline   4 Projets   5 Cache                                      │
├──────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                  │
│   COUT API TOTAL ESTIME        $ 347.82          Periode    2026-02-14  ->  2026-05-30 (105 j)   │
│   Messages factures            18 432            (42 100 lignes scannees, 23 668 doublons)       │
│                                                                                                  │
│   Tokens   input          14.2 M     output        3.8 M                                         │
│            cache write    61.7 M     cache read   402.5 M                                        │
│                                                                                                  │
│   Fenetres   Aujourd'hui   $   4.18      7 jours   $  38.07      30 jours   $ 121.55             │
│                                                                                                  │
│   Economie cache   ~ 81 %  servi par cache         $ 188.40 economises vs sans cache             │
│                                                                                                  │
│   Top projets   my-saas-api $142.10 · client-dashboard $98.65 · side-project $71.20 · docs $35.87│
│                                                                                                  │
│   ! ESTIMATION au tarif API a la demande, PAS la facture reelle d'un abonnement Max/Pro.         │
│                                                                                                  │
├──────────────────────────────────────────────────────────────────────────────────────────────┤
│  Tab/<->  vue    1-5  vue directe    j/k  defiler    q  quitter                                  │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
```

## Install & Run

You need a [Rust toolchain](https://rustup.rs/) (2024 edition).

```bash
git clone https://github.com/sanztheo/cc-cost.git
cd cc-cost

cargo run                 # build, scan, and open the TUI
cargo run -- --report     # plain-text report, no TUI
cargo run -- --snapshot   # render all five views as text (works outside a TTY)
```

### Install as a global command

```bash
cargo install --path .
```

This drops a `cc-cost` binary in `~/.cargo/bin` (already on your `PATH` if you use rustup), so you can run `cc-cost` from anywhere — no `cargo run`, no rebuild. Want a different name? Add a shell alias, e.g. `alias claude-cost="cc-cost"`.

```bash
cc-cost              # TUI, from any directory
cc-cost --report     # text report
cc-cost --snapshot   # text snapshot of all views
```

The first run scans your local Claude Code data. On a ~2.6 GB transcript history this takes roughly 2 seconds thanks to parallel parsing.

## Usage

Keybindings inside the TUI:

| Key | Action |
| --- | --- |
| `1`–`5` / `n` `p` | Switch view (numbers jump directly; `n`/`p` cycle next/prev) |
| `t` / `T` | Time window — `t` cycles forward, `T` backward (Today · 7d · 14d · 30d · 90d · All) |
| `w` | Timeline daily ↔ weekly (jumps to the Timeline view) |
| `j` / `k` | Scroll within the current view |
| `q` | Quit |

> All shortcuts are plain letters and numbers — no `Option`/`Alt`, so they work the same on AZERTY and QWERTY. `Tab` and the arrow keys also navigate, but a terminal multiplexer (tmux, cmux, screen…) may swallow them; the letter/number keys never get intercepted.

The selected time window applies to **every** view at once — press `t` to reach `30d` and the totals, model breakdown, timeline, projects, and cache savings all recompute for the last 30 days. The Overview also keeps fixed all-time reference points (today / 7d / 30d) and a 30-day run-rate projection.

### Where it looks

`cc-cost` scans, in order of precedence:

- the paths listed in `CLAUDE_CONFIG_DIR` (colon-separated, like `PATH`), if set; otherwise
- `~/.claude/projects` and `~/.config/claude/projects`.

Symlinks are **not** followed — this is deliberate, to avoid counting the same transcript twice when directories are linked together.

## How cost is calculated

The cost is recomputed from the token counts recorded in each message, never read from a precomputed field.

### Pricing table

Official Anthropic rates in **USD per million tokens**, verified 2026-05-30:

| Model | Input | Output | Cache write (5m) | Cache write (1h) | Cache read |
| --- | ---: | ---: | ---: | ---: | ---: |
| Opus 4.6 / 4.7 / 4.8 | 5 | 25 | 6.25 | 10 | 0.5 |
| Sonnet 4.6 | 3 | 15 | 3.75 | 6 | 0.3 |
| Haiku 4.5 | 1 | 5 | 1.25 | 2 | 0.1 |
| Opus 4 / 4.1 (legacy) | 15 | 75 | 18.75 | 30 | 1.5 |

### Four token types, billed separately

Each message is priced across four independently-billed token categories:

- **Input** — billed at the model's input rate.
- **Output** — billed at the output rate.
- **Cache write** — split by the cache's time-to-live. 5-minute ephemeral writes are billed at **1.25×** the input rate and 1-hour writes at **2×** the input rate, read from the `ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` fields respectively.
- **Cache read** — billed at **0.1×** the input rate (the discount the Cache view quantifies).

### Deduplication

This is the part that matters most for accuracy. Claude Code rewrites the same messages into different files — running `claude --continue`, for example, re-emits earlier turns elsewhere in the transcript tree. In a typical history **~58% of message lines are duplicates**, so naïvely summing every line inflates the cost by roughly **2.4×**.

`cc-cost` deduplicates on the `(message.id, requestId)` pair, counting each real request exactly once. It also excludes messages from the `<synthetic>` model, which represent local placeholders and cost nothing.

## Accuracy & limitations

**This is an estimate, not a bill.** `cc-cost` tells you what your usage *would have cost at pay-as-you-go API rates*. It is **not** the invoice for a Claude Max or Pro subscription, and the two are not meant to match — that gap is the entire point of the tool. Treat the numbers as "what the API meter would have read," nothing more.

**Multi-account histories cannot be split.** If you run multiple Claude Code accounts under different `CLAUDE_CONFIG_DIR` paths, and those directories symlink their `projects/` folder back into a shared `~/.claude`, every account writes into the same pool of transcripts. `cc-cost` will scan all of it (and count it correctly, once), but it cannot attribute cost per account: the JSONL transcripts carry no account tag, so there is nothing to split on.

## Tech stack

- **Rust** (2024 edition)
- **[ratatui](https://ratatui.rs/) 0.30** — the terminal UI
- **serde / serde_json** — transcript parsing
- **rayon** — parallel parsing across files
- **walkdir** — filesystem traversal (deliberately not following symlinks)
- **chrono** — date and time-window handling
- **anyhow** — error handling

100% local, read-only, no network access.

## Contributing

Issues and pull requests are welcome. If you're reporting a pricing discrepancy, please include the model and the relevant token categories so the rate table can be checked against the current Anthropic tariff. Keep changes focused, and run `cargo fmt` and `cargo clippy` before opening a PR.

## License

[MIT](LICENSE) © contributors. Repository: [github.com/sanztheo/cc-cost](https://github.com/sanztheo/cc-cost).
