# Recap

A developer productivity dashboard that aggregates your activity across **GitHub**, **Linear**, **Slack**, and **Notion** into a bento-box style UI with charts, AI summaries, and standup generation.

Built with Rust and Tauri v2 for minimal resource usage (~10MB RAM, no Electron, no Node.js).

## Features

- **Bento-box dashboard** — 2-column grid with charts, progress bars, and activity tables
- **Activity Over Time chart** — stacked bar chart of merges, reviews, commits, issues, messages per day
- **PR Stats** — doughnut chart with opened/merged/reviewed breakdown
- **Feature Area breakdown** — horizontal segmented bars per project, colored by activity kind
- **Linear Progress** — completed/in-progress/other bar visualization
- **AI Daily Briefing** — auto-generated summary via `claude` CLI, grouped by theme not tool
- **Standup Generator** — "What I Did / What I Will Do" with copy-to-clipboard
- **Day / Week / Month views** — all cards and charts update per period
- **Background sync** every 5 minutes (configurable) with incremental fetching
- **SQLite caching** with tiered TTL (5 min hot / 1 hour warm / 24 hour cold)
- **GitHub PR search** — catches PRs merged by Graphite/bots via the Search API
- **Slack channel filtering** with glob patterns (e.g., `graphite-*`, `github-prs`)
- **GitHub auto-detection** — picks up your username and token from the `gh` CLI
- **macOS Keychain** storage for all API tokens
- **Daily reminder notification** at a configurable time (default: 5pm)
- **System tray** with context menu (Open, Sync Now, Quit)

## Quick Start

### Prerequisites

- Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- macOS (uses native WebKit webview and Keychain)
- [GitHub CLI](https://cli.github.com/) (`brew install gh && gh auth login`) — recommended for zero-config GitHub auth
- [Claude Code](https://claude.ai/download) — for AI briefings and standup generation (optional but recommended)

### Build & Run

```bash
cd recap
cargo run
```

On first launch, Recap creates a default config at `~/Library/Application Support/recap/config.toml` and opens the dashboard. If any services are disconnected, a banner at the top links to the setup modal.

## Connecting Services

| Service | How to connect |
|---------|---------------|
| **GitHub** | Automatic if `gh` CLI is authenticated. Otherwise, paste a [Personal Access Token](https://github.com/settings/tokens) with `repo` and `read:user` scopes. |
| **Linear** | Grab a Personal API key from **Settings > API > Personal API keys** in Linear. |
| **Slack** | Create a Slack app from the included manifest (see below), install to your workspace, and paste the `xoxp-` user token. If token rotation is enabled, also save the refresh token. |

### Slack App Setup

1. Go to [api.slack.com/apps](https://api.slack.com/apps) and click **Create New App > From a manifest**
2. Paste the contents of `slack-app-manifest.yaml` from this repo
3. Click **Create**, then **Install to Workspace**
4. Copy the **User OAuth Token** (`xoxp-...` or `xoxe.xoxp-...`) from **OAuth & Permissions**
5. Paste it into Recap's setup modal

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
- Search API (`/search/issues?q=author:{username}`) — catches PRs merged by Graphite or other bots

**Rate limit handling:** Each integration detects 429/rate-limit responses and backs off with the `Retry-After` header value.

You can also hit the **Sync** button in the header to trigger an immediate sync of all sources.

## AI Features

Recap uses the `claude` CLI (Claude Code) for AI-powered features — no API key needed if you have Claude Code installed.

| Feature | How it works |
|---------|-------------|
| **Daily Briefing** | Sends your activities to `claude --print` with a prompt to summarize by theme. Auto-fetches on dashboard load. Cached for 1 hour. |
| **Standup Generator** | Sends yesterday's + today's activities to `claude --print` with a standup prompt. Produces "What I Did / What I Will Do" sections. Copy button for clipboard. |

Falls back to the Anthropic API if `claude` CLI isn't available (requires an API key in Keychain).

## Configuration

Edit `~/Library/Application Support/recap/config.toml`:

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

[slack]
# user_id = "U..."              # Your Slack user ID
# ignored_channels = ["github-prs", "graphite-*", "dependabot-*"]
# client_id = "..."             # Required for token refresh
# client_secret = "..."         # Required for token refresh

[llm]
enabled = false                  # Set to true for Anthropic API fallback
# model = "claude-haiku-4-5-20251001"
```

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Recap Dashboard (Tauri v2, native WebKit webview)           │
│  ┌─────────────────────────┬────────────────────────────┐    │
│  │ Daily Briefing (AI)     │ Activity Over Time (chart) │    │
│  ├─────────────────────────┼────────────────────────────┤    │
│  │ Standup Generator       │ PR Stats (donut)           │    │
│  ├─────────────────────────┼────────────────────────────┤    │
│  │ Feature Areas (bars)    │ Linear Progress (bars)     │    │
│  ├─────────────────────────┴────────────────────────────┤    │
│  │ Recent Activity (table with links)                    │    │
│  └───────────────────────────────────────────────────────┘    │
└──────────────────────────┬───────────────────────────────────┘
                           │ Tauri IPC (10 commands)
┌──────────────────────────┴───────────────────────────────────┐
│  Rust Backend                                                 │
│  ├── Sync Scheduler (tokio, concurrent, 5-min interval)      │
│  │   ├── GitHub  (Events API + Search API for Graphite PRs)  │
│  │   ├── Linear  (GraphQL, issue history + state transitions)│
│  │   └── Slack   (search.messages, token refresh support)    │
│  ├── LLM (claude CLI primary, Anthropic API fallback)        │
│  ├── SQLite (rusqlite, WAL mode, tiered TTL cache)           │
│  │   ├── Activities (unified schema, deduplicated)           │
│  │   ├── Sync cursors (incremental fetching)                 │
│  │   └── LLM/standup cache                                   │
│  ├── Notifications (daily digest at configurable time)       │
│  └── Auth (macOS Keychain via keyring crate)                 │
└──────────────────────────────────────────────────────────────┘
```

## Project Structure

```
recap/
├── src/
│   ├── main.rs              # Entry point
│   ├── lib.rs               # App bootstrap (Tauri setup, scheduler, notifications)
│   ├── config.rs            # Config from ~/Library/Application Support/recap/
│   ├── auth.rs              # macOS Keychain credential storage
│   ├── tray.rs              # System tray icon + context menu
│   ├── commands.rs          # Tauri IPC commands (10 endpoints)
│   ├── llm.rs               # Claude CLI + Anthropic API fallback
│   ├── notifications.rs     # Daily reminder scheduler
│   ├── models/
│   │   └── activity.rs      # Activity, Source, ActivityKind, Digest, Period
│   ├── db/
│   │   ├── migrations.rs    # SQLite schema
│   │   ├── cache.rs         # TTL cache logic
│   │   └── queries.rs       # Activity CRUD + sync cursors
│   ├── integrations/
│   │   ├── mod.rs           # Integration trait
│   │   ├── github.rs        # GitHub Events + Search APIs
│   │   ├── linear.rs        # Linear GraphQL
│   │   ├── slack.rs         # Slack Web API + token refresh
│   │   └── notion.rs        # Notion Search API
│   ├── sync/
│   │   └── scheduler.rs     # Background sync orchestration
│   └── digest/
│       └── daily.rs         # Digest aggregation
├── ui/
│   ├── index.html           # Bento-box dashboard
│   ├── style.css            # Dark theme, CSS Grid, responsive
│   └── app.js               # Vanilla JS + Chart.js
├── config.example.toml
├── slack-app-manifest.yaml
└── Cargo.toml
```

## Data Storage

All data stays local on your machine:

| What | Where |
|------|-------|
| Config | `~/Library/Application Support/recap/config.toml` |
| Database | `~/Library/Application Support/recap/recap.db` |
| API tokens | macOS Keychain (service: `com.recap.app`) |

## License

MIT
