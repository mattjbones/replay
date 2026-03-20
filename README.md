# Recap

A cross-platform developer productivity dashboard that aggregates your activity across **GitHub** and **Linear** into a bento-box style UI with charts, AI summaries, and standup generation.

Built with Rust and Tauri v2 for minimal resource usage (~10MB RAM, no Electron, no Node.js). Runs on **macOS**, **Linux**, and **Windows**.

## Features

- **Bento-box dashboard** — 2-column grid with headline metrics, charts, progress bars, and activity tables
- **Tabbed navigation** — Overview | GitHub | Linear with source-specific breakout views
- **Activity Over Time chart** — stacked bar chart of merges, reviews, commits, issues per day
- **PR Stats** — doughnut chart with opened/merged/reviewed breakdown
- **Conventional Commit tags** — parsed from PR titles (`feat`, `fix`, `perf`, `chore`, etc.) with filter bar
- **Graphite merge queue support** — detects PRs merged via Graphite (branch deletion check)
- **Feature Area breakdown** — horizontal segmented bars per project, colored by activity kind
- **Linear Progress** — completed/in-progress/other bar visualization with state filters
- **AI Daily Briefing** — auto-generated summary via `claude` CLI, grouped by theme not tool
- **Standup Generator** — "What I Did / What I Will Do" modal with copy-to-clipboard
- **Date navigation** — prev/next arrows to browse historical day/week/month data
- **Background sync** every 5 minutes (configurable) with incremental fetching
- **SQLite caching** with tiered TTL (5 min hot / 1 hour warm / 24 hour cold)
- **Settings modal** — manage connections + preferences (sync interval, ignored channels, etc.)
- **GitHub auto-detection** — picks up your username and token from the `gh` CLI
- **Token storage in SQLite** — no OS keychain dependency, works identically on all platforms
- **Daily reminder notification** at a configurable time (default: 5pm)
- **System tray** with context menu (Open, Sync Now, Quit)

## Quick Start

### Prerequisites

- **Rust toolchain**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Platform dependencies**:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Linux**: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` (Debian/Ubuntu) or equivalent
  - **Windows**: [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (usually pre-installed on Windows 10+)
- **[GitHub CLI](https://cli.github.com/)**: `gh auth login` — recommended for zero-config GitHub auth
- **[Claude Code](https://claude.ai/download)** — for AI briefings and standup generation (optional but recommended)

### Build & Run

```bash
cd recap
cargo run
```

Or on macOS with ad-hoc signing (avoids repeated Keychain prompts during dev):

```bash
make dev
```

On first launch, Recap creates a default config and opens the dashboard. If any services are disconnected, click **Settings** in the action bar to add tokens.

## Connecting Services

| Service | How to connect |
|---------|---------------|
| **GitHub** | Automatic if `gh` CLI is authenticated. Otherwise, paste a [Personal Access Token](https://github.com/settings/tokens) with `repo` and `read:user` scopes via Settings. |
| **Linear** | Grab a Personal API key from **Settings > API > Personal API keys** in Linear. Paste via Settings. |

## Sync & Polling

Recap syncs data from all connected services on a background loop:

| Setting | Default | Description |
|---------|---------|-------------|
| `sync_interval_minutes` | **5** | How often the scheduler wakes up to check for new data |
| `ttl.hot_minutes` | **5** | Cache TTL for today's data — if less than 5 min since last sync, skip |
| `ttl.warm_minutes` | **60** | Cache TTL for this week's data |
| `ttl.cold_minutes` | **1440** | Cache TTL for older data (24 hours) |

**How it works:**

1. A tokio background task wakes every `sync_interval_minutes` (default 5 min)
2. For each connected integration, it checks the `sync_cursors` table for the last sync time
3. If the cache is still fresh (within TTL for the data's age), that source is skipped
4. Stale sources are fetched **concurrently** via `tokio::JoinSet`
5. Each integration uses incremental cursors — only fetching events/items newer than the last sync
6. New activities are upserted into SQLite (deduped by `source + source_id`)
7. The UI does **not** poll — it fetches on load, tab switch, or manual sync

**GitHub uses two data sources per sync:**
- Events API (`/users/{username}/events`) — pushes, reviews, issues
- Search API (`/search/issues?q=author:{username}`) — all authored PRs with merge status
- Graphite merge detection — for closed PRs without `merged_at`, checks if the head branch was deleted (parallel via `JoinSet`)

**Rate limit handling:** Each integration detects 429/rate-limit responses and backs off with the `Retry-After` header value.

## AI Features

Recap uses the `claude` CLI (Claude Code) for AI-powered features — no API key needed if you have Claude Code installed.

| Feature | How it works |
|---------|-------------|
| **Daily Briefing** | Sends your activities to `claude --print` with a prompt to summarize by theme. Auto-fetches on dashboard load. Cached for 1 hour. |
| **Standup Generator** | Sends yesterday's + today's activities to `claude --print` with a standup prompt. Produces "What I Did / What I Will Do" sections. Copy button for clipboard. |

Falls back to the Anthropic API if `claude` CLI isn't available (requires an API key stored via Settings).

## Configuration

Config file location (created automatically on first run):

| Platform | Path |
|----------|------|
| **macOS** | `~/Library/Application Support/recap/config.toml` |
| **Linux** | `~/.config/recap/config.toml` |
| **Windows** | `%APPDATA%\recap\config.toml` |

You can also edit preferences via the **Settings** modal in the app.

```toml
[schedule]
sync_interval_minutes = 5        # Background sync interval
daily_reminder_time = "17:00"    # Daily notification time
weekly_reminder_day = "Friday"

[ttl]
hot_minutes = 5                  # Cache TTL for today's data
warm_minutes = 60                # Cache TTL for this week's data
cold_minutes = 1440              # Cache TTL for older data (24h)

[github]
# username = "your-username"     # Auto-detected from `gh` CLI if omitted

[llm]
enabled = false                  # Set to true for Anthropic API fallback
# model = "claude-haiku-4-5-20251001"
```

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Recap Dashboard (Tauri v2, native platform webview)          │
│  ┌──────────────────────────────────────────────────────┐    │
│  │ Overview │ GitHub │ Linear    [< Date >] [D│W│M]     │    │
│  ├─────────────────────────┬────────────────────────────┤    │
│  │ [Actions: Standup, Settings, Sync]                    │    │
│  ├─────────────────────────┬────────────────────────────┤    │
│  │ Headline Metrics (4x)   │                            │    │
│  ├─────────────────────────┼────────────────────────────┤    │
│  │ Daily Briefing (AI)     │ Activity Over Time (chart) │    │
│  ├─────────────────────────┼────────────────────────────┤    │
│  │ PR Stats (donut)        │ Feature Areas (bars)       │    │
│  ├─────────────────────────┴────────────────────────────┤    │
│  │ Recent Activity (table with links + filters)          │    │
│  └───────────────────────────────────────────────────────┘    │
└──────────────────────────┬───────────────────────────────────┘
                           │ Tauri IPC (12 commands)
┌──────────────────────────┴───────────────────────────────────┐
│  Rust Backend                                                 │
│  ├── Sync Scheduler (tokio, concurrent, 5-min interval)      │
│  │   ├── GitHub  (Events + Search APIs, Graphite detection)  │
│  │   └── Linear  (GraphQL, issue history + state transitions)│
│  ├── LLM (claude CLI primary, Anthropic API fallback)        │
│  ├── SQLite (rusqlite, WAL mode, tiered TTL cache)           │
│  │   ├── Activities (unified schema, deduplicated)           │
│  │   ├── Sync cursors (incremental fetching)                 │
│  │   ├── Tokens (stored in DB, no OS keychain needed)        │
│  │   └── LLM/standup cache                                   │
│  ├── Notifications (daily digest at configurable time)       │
│  └── Auth (SQLite-backed, GitHub auto-detected from gh CLI)  │
└──────────────────────────────────────────────────────────────┘
```

## Platform Support

| Platform | Webview | Status |
|----------|---------|--------|
| **macOS** | WebKit (native) | Primary development platform |
| **Linux** | WebKitGTK | Supported — requires `libwebkit2gtk-4.1-dev` |
| **Windows** | WebView2 | Supported — usually pre-installed |

Platform-specific behavior:
- **macOS**: `ActivationPolicy::Regular` set for proper event dispatch; `codesign` in `make dev` for Keychain compatibility; tray icon uses template mode
- **Linux/Windows**: macOS-specific code is gated behind `#[cfg(target_os = "macos")]` and ignored
- **Config paths**: handled by the `dirs` crate (platform-appropriate locations)
- **Tokens**: stored in SQLite (no OS keychain dependency)
- **TLS**: uses `rustls` (no OpenSSL dependency on any platform)

## Project Structure

```
recap/
├── src/
│   ├── main.rs              # Entry point
│   ├── lib.rs               # App bootstrap (Tauri setup, scheduler, notifications)
│   ├── config.rs            # Config (platform-appropriate paths via dirs crate)
│   ├── auth.rs              # SQLite-backed token storage
│   ├── tray.rs              # System tray icon + context menu
│   ├── commands.rs          # Tauri IPC commands (12 endpoints)
│   ├── llm.rs               # Claude CLI + Anthropic API fallback
│   ├── notifications.rs     # Daily reminder scheduler
│   ├── models/
│   │   └── activity.rs      # Activity, Source, ActivityKind, Digest, Period
│   ├── db/
│   │   ├── migrations.rs    # SQLite schema (activities, cursors, tokens, cache)
│   │   ├── cache.rs         # TTL cache logic
│   │   └── queries.rs       # Activity CRUD + sync cursors
│   ├── integrations/
│   │   ├── mod.rs           # Integration trait
│   │   ├── github.rs        # GitHub Events + Search APIs + Graphite detection
│   │   ├── linear.rs        # Linear GraphQL
│   │   ├── slack.rs         # Slack Web API (disabled, requires OAuth)
│   │   └── notion.rs        # Notion Search API (disabled, requires app)
│   ├── sync/
│   │   └── scheduler.rs     # Background sync orchestration
│   └── digest/
│       └── daily.rs         # Digest aggregation
├── ui/
│   ├── index.html           # Tabbed dashboard with modals
│   ├── style.css            # Dark theme, CSS Grid, CSS vars, responsive
│   └── app.js               # Vanilla JS + Chart.js
├── Makefile                 # dev/run/release/clean targets
├── config.example.toml
└── Cargo.toml
```

## Data Storage

All data stays local on your machine:

| What | macOS | Linux | Windows |
|------|-------|-------|---------|
| Config | `~/Library/Application Support/recap/` | `~/.config/recap/` | `%APPDATA%\recap\` |
| Database | same directory, `recap.db` | same | same |
| API tokens | inside `recap.db` (tokens table) | same | same |

## License

MIT
