# Recap

A lightweight macOS menubar app that aggregates your activity across **GitHub**, **Linear**, **Slack**, and **Notion** into daily, weekly, and monthly digests.

Built with Rust and Tauri v2 for minimal resource usage (~10MB RAM, no Electron, no Node.js).

## Features

- **Unified activity feed** from four sources, normalized into a single timeline
- **Day / Week / Month views** with per-source breakdowns and activity counts
- **Background sync** on a configurable interval (default: 5 minutes) with incremental fetching
- **SQLite caching** with tiered TTL (5 min for today, 1 hour for this week, 24 hours for older data)
- **AI summaries** (optional) via Claude API — groups your work by theme, not by tool
- **Daily reminder notification** at a configurable time (default: 5pm)
- **System tray** with left-click popup and right-click context menu (Open, Sync Now, Quit)
- **macOS Keychain** storage for all API tokens — nothing in plaintext
- **Slack channel filtering** with glob patterns (e.g., `graphite-*`, `github-prs`)
- **GitHub auto-detection** — picks up your username and token from the `gh` CLI

## Quick Start

### Prerequisites

- Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- macOS (uses native WebKit webview and Keychain)
- [GitHub CLI](https://cli.github.com/) (`brew install gh && gh auth login`) — recommended for zero-config GitHub auth

### Build & Run

```bash
cd recap
cargo run
```

On first launch, Recap creates a default config at `~/.config/recap/config.toml` and opens the setup wizard in the tray popup to connect your services.

### Connecting Services

| Service | How to connect |
|---------|---------------|
| **GitHub** | Automatic if `gh` CLI is authenticated. Otherwise, paste a [Personal Access Token](https://github.com/settings/tokens) with `repo` and `read:user` scopes. |
| **Linear** | Grab a Personal API key from **Settings > API > Personal API keys** in Linear. |
| **Slack** | Create a Slack app from the included manifest (see below), install to your workspace, and paste the `xoxp-` user token. |
| **Notion** | Create an [internal integration](https://www.notion.so/my-integrations), then paste the token. Share any pages/databases you want tracked with the integration. |

#### Slack App Setup

1. Go to [api.slack.com/apps](https://api.slack.com/apps) and click **Create New App > From a manifest**
2. Paste the contents of `slack-app-manifest.yaml` from this repo
3. Click **Create**, then **Install to Workspace**
4. Copy the **User OAuth Token** (`xoxp-...`) from **OAuth & Permissions**
5. Paste it into Recap's setup wizard or save it via the tray UI

### Configuration

Edit `~/.config/recap/config.toml`:

```toml
[schedule]
sync_interval_minutes = 5        # How often to fetch new data
daily_reminder_time = "17:00"    # When to show the daily digest notification
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

[llm]
enabled = false                  # Set to true to enable AI summaries
# model = "claude-haiku-4-5-20251001"
```

### AI Summaries

To enable LLM-powered summaries of your activity:

1. Store your Anthropic API key in the macOS Keychain (Recap will prompt via the UI, or use the Keychain Access app with service `com.recap.app` and account `anthropic_api_key`)
2. Set `llm.enabled = true` in your config
3. Click the **AI Summary** dropdown in the digest view

Summaries are cached for 1 hour to avoid redundant API calls. Cost is fractions of a cent per summary using Haiku.

## Architecture

```
┌──────────────────────────────────────────────────┐
│  macOS Menu Bar (Tauri v2 system tray)           │
│  ┌──────────────────────────────────────────┐    │
│  │  Popup Window (native WebKit webview)    │    │
│  │  - Day / Week / Month tabs               │    │
│  │  - Activity feed by source               │    │
│  │  - AI Summary dropdown                   │    │
│  └──────────────────────────────────────────┘    │
└──────────────────────┬───────────────────────────┘
                       │ Tauri IPC
┌──────────────────────┴───────────────────────────┐
│  Rust Backend                                     │
│  ├── Sync Scheduler (tokio, concurrent)           │
│  │   ├── GitHub  (REST Events API)                │
│  │   ├── Linear  (GraphQL)                        │
│  │   ├── Slack   (search.messages)                │
│  │   └── Notion  (Search API)                     │
│  ├── SQLite (rusqlite, WAL mode)                  │
│  │   ├── Activities table (unified schema)        │
│  │   ├── Sync cursors (incremental fetch)         │
│  │   └── LLM cache (TTL-based)                    │
│  ├── Notifications (daily digest reminder)        │
│  └── Auth (macOS Keychain via keyring crate)      │
└──────────────────────────────────────────────────┘
```

## Project Structure

```
recap/
├── src/
│   ├── main.rs              # Entry point
│   ├── lib.rs               # App bootstrap (Tauri setup, scheduler, notifications)
│   ├── config.rs            # Config from ~/.config/recap/config.toml
│   ├── auth.rs              # macOS Keychain credential storage
│   ├── tray.rs              # System tray icon + context menu
│   ├── commands.rs          # Tauri IPC commands (7 endpoints)
│   ├── llm.rs               # Claude API integration
│   ├── notifications.rs     # Daily reminder scheduler
│   ├── models/
│   │   └── activity.rs      # Activity, Source, ActivityKind, Digest, Period
│   ├── db/
│   │   ├── migrations.rs    # SQLite schema
│   │   ├── cache.rs         # TTL cache logic
│   │   └── queries.rs       # Activity CRUD + sync cursors
│   ├── integrations/
│   │   ├── mod.rs           # Integration trait
│   │   ├── github.rs        # GitHub Events API
│   │   ├── linear.rs        # Linear GraphQL
│   │   ├── slack.rs         # Slack Web API
│   │   └── notion.rs        # Notion Search API
│   ├── sync/
│   │   └── scheduler.rs     # Background sync orchestration
│   └── digest/
│       └── daily.rs         # Digest aggregation
├── ui/
│   ├── index.html           # Single-page app
│   ├── style.css            # Dark theme
│   └── app.js               # Vanilla JS frontend (~200 LOC)
├── config.example.toml
├── slack-app-manifest.yaml
└── Cargo.toml
```

## Data Storage

All data stays local on your machine:

| What | Where |
|------|-------|
| Config | `~/.config/recap/config.toml` |
| Database | `~/.config/recap/recap.db` |
| API tokens | macOS Keychain (service: `com.recap.app`) |

## License

MIT
