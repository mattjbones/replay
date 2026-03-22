// tauri-mock.js -- Injected into the page before app.js runs.
// Stubs window.__TAURI__.core.invoke() and other Tauri APIs with fixture data
// so the UI can be tested in a normal browser without the Tauri runtime.

(() => {
  // -----------------------------------------------------------------------
  // Fixture data
  // -----------------------------------------------------------------------

  const NOW = new Date().toISOString();
  const TODAY = NOW.slice(0, 10);

  function makeActivity(overrides) {
    return {
      id: "01J" + Math.random().toString(36).slice(2, 10).toUpperCase(),
      source: "github",
      source_id: "gh-" + Math.floor(Math.random() * 10000),
      kind: "commit_pushed",
      title: "Fix widget rendering",
      description: null,
      url: "https://github.com/org/repo/commit/abc123",
      project: "recap",
      occurred_at: NOW,
      metadata: {},
      synced_at: NOW,
      ...overrides,
    };
  }

  const FIXTURE_ACTIVITIES = [
    makeActivity({ kind: "pr_merged", title: "feat: add dashboard grid layout", project: "recap", source: "github", metadata: { cc_type: "feat" } }),
    makeActivity({ kind: "pr_opened", title: "fix: date navigation off-by-one", project: "recap", source: "github", metadata: { cc_type: "fix" } }),
    makeActivity({ kind: "pr_reviewed", title: "chore: update dependencies", project: "recap", source: "github" }),
    makeActivity({ kind: "commit_pushed", title: "refactor auth module", project: "recap", source: "github" }),
    makeActivity({ kind: "commit_pushed", title: "add unit tests for digest", project: "recap", source: "github" }),
    makeActivity({ kind: "issue_created", title: "Implement Slack OAuth flow", project: "recap", source: "linear", source_id: "LIN-101" }),
    makeActivity({ kind: "issue_completed", title: "Fix settings modal styling", project: "recap", source: "linear", source_id: "LIN-102" }),
    makeActivity({ kind: "issue_updated", title: "Redesign trends page", project: "recap", source: "linear", source_id: "LIN-103" }),
    makeActivity({ kind: "pr_merged", title: "feat: add trend forecasting", project: "analytics", source: "github", metadata: { cc_type: "feat" } }),
    makeActivity({ kind: "pr_reviewed", title: "docs: update README", project: "docs", source: "github" }),
  ];

  const FIXTURE_DIGEST = {
    period: { Day: TODAY },
    activities: FIXTURE_ACTIVITIES,
    stats: {
      total_activities: FIXTURE_ACTIVITIES.length,
      by_source: { github: 7, linear: 3 },
      by_kind: {
        pr_merged: 2,
        pr_opened: 1,
        pr_reviewed: 2,
        commit_pushed: 2,
        issue_created: 1,
        issue_completed: 1,
        issue_updated: 1,
      },
    },
    llm_summary: null,
  };

  const FIXTURE_CHART_DATA = {
    labels: ["Mon 01", "Tue 02", "Wed 03", "Thu 04", "Fri 05"],
    datasets: {
      merges: [1, 0, 1, 0, 0],
      reviews: [0, 1, 0, 1, 0],
      commits: [2, 1, 0, 1, 1],
      issues: [1, 0, 1, 1, 0],
      messages: [0, 0, 0, 0, 0],
    },
  };

  const FIXTURE_FEATURES = [
    { project: "recap", count: 7, kinds: { pr_merged: 1, commit_pushed: 2, pr_reviewed: 1, issue_created: 1, issue_completed: 1, issue_updated: 1 } },
    { project: "analytics", count: 1, kinds: { pr_merged: 1 } },
    { project: "docs", count: 1, kinds: { pr_reviewed: 1 } },
  ];

  const FIXTURE_AUTH_STATUS = {
    github: true,
    linear: true,
    slack: true,
    notion: true,
    anthropic: false,
  };

  const FIXTURE_CONFIG = {
    schedule: { sync_interval_minutes: 5, daily_reminder_time: "17:00", weekly_reminder_day: "Friday" },
    ttl: { hot_minutes: 5, warm_minutes: 60, cold_minutes: 1440 },
    github: { username: "testuser", workflow: "pr" },
    linear: {},
    slack: { user_id: null, ignored_channels: [], client_id: null, client_secret: null },
    notion: {},
    llm: { enabled: false, model: "claude-haiku-4-5-20251001" },
    working_hours: { work_start: "09:00", work_end: "17:00", working_days: ["Mon", "Tue", "Wed", "Thu", "Fri"], timezone: "UTC" },
    dashboard_layout: {},
  };

  const FIXTURE_OPEN_TICKETS = [
    { identifier: "REC-42", title: "Implement heatmap drill-down", url: "https://linear.app/team/REC-42", state: "In Progress", state_type: "started", priority: 1, priority_label: "Urgent", team: "Engineering", created_at: NOW, updated_at: NOW },
    { identifier: "REC-43", title: "Add export to CSV", url: "https://linear.app/team/REC-43", state: "Todo", state_type: "unstarted", priority: 2, priority_label: "High", team: "Engineering", created_at: NOW, updated_at: NOW },
  ];

  const FIXTURE_OPEN_PRS = [
    { number: 101, title: "feat: add dark mode toggle", url: "https://github.com/org/recap/pull/101", repo: "recap", state: "open", created_at: NOW, updated_at: NOW, labels: ["enhancement"], review_status: "review_required", additions: 45, deletions: 12 },
    { number: 99, title: "WIP: slack integration", url: "https://github.com/org/recap/pull/99", repo: "recap", state: "draft", created_at: NOW, updated_at: NOW, labels: ["wip"], review_status: "", additions: 200, deletions: 30 },
  ];

  const FIXTURE_GITHUB_ISSUES = [
    { number: 55, title: "Settings page layout broken on small screens", url: "https://github.com/org/recap/issues/55", repo: "recap", state: "open", created_at: NOW, updated_at: NOW, labels: ["bug"], comments: 3 },
    { number: 50, title: "Add keyboard shortcuts", url: "https://github.com/org/recap/issues/50", repo: "recap", state: "open", created_at: NOW, updated_at: NOW, labels: ["enhancement"], comments: 1 },
  ];

  const FIXTURE_TRENDS_DATA = {
    velocity: {
      weeks: ["2026-W10", "2026-W11", "2026-W12"],
      series: {
        pr_merged: [3, 5, 4],
        issue_completed: [2, 4, 3],
        commit_pushed: [10, 12, 8],
        pr_reviewed: [4, 3, 5],
      },
      trend_slopes: { pr_merged: 0.5, issue_completed: 0.5, commit_pushed: -1.0, pr_reviewed: 0.5 },
    },
    heatmap: [
      { day: 1, hour: 10, count: 5 },
      { day: 1, hour: 14, count: 8 },
      { day: 2, hour: 11, count: 3 },
    ],
    cycle_time: [
      { week: "2026-W10", avg_hours: 24.5 },
      { week: "2026-W11", avg_hours: 18.2 },
      { week: "2026-W12", avg_hours: 20.1 },
    ],
    focus: {
      weeks: ["2026-W10", "2026-W11", "2026-W12"],
      projects: { recap: [5, 8, 6], analytics: [2, 1, 3] },
      fragmentation_index: [2, 2, 2],
    },
    prediction: {
      weeks_ahead: ["W+1", "W+2", "W+3"],
      forecasts: { pr_merged: [4.5, 4.8, 5.0], issue_completed: [3.5, 3.8, 4.0] },
      confidence: "medium",
    },
    burnout: {
      weeks: ["2026-W10", "2026-W11", "2026-W12"],
      off_hours_pct: [10.0, 12.5, 8.0],
      message_volume: [20, 25, 18],
      trend_direction: "stable",
    },
    anomalies: [],
    day_clusters: {
      clusters: [
        { name: "Coding Day", centroid: [8.0, 1.0, 0.5, 0.5, 0.0], count: 5 },
        { name: "Reviews Day", centroid: [2.0, 0.5, 4.0, 1.0, 0.0], count: 3 },
        { name: "Issues Day", centroid: [1.0, 0.0, 0.5, 3.0, 2.0], count: 2 },
      ],
      days: [
        { date: "2026-03-16", cluster: "Coding Day" },
        { date: "2026-03-17", cluster: "Reviews Day" },
      ],
    },
    project_prediction: [
      { project: "recap", probability: 0.65 },
      { project: "analytics", probability: 0.25 },
    ],
    productivity: {
      weeks: ["2026-W10", "2026-W11", "2026-W12"],
      scores: [18.5, 24.0, 21.5],
      current_score: 21.5,
      trend: "improving",
      baseline_avg: 21.3,
    },
  };

  // -----------------------------------------------------------------------
  // IPC command router
  // -----------------------------------------------------------------------

  const COMMANDS = {
    get_digest: () => FIXTURE_DIGEST,
    get_auth_status: () => FIXTURE_AUTH_STATUS,
    save_token: () => null,
    save_slack_refresh_token: () => null,
    save_anthropic_key: () => null,
    exchange_slack_refresh_token: () => "Slack connected! Access token starts with xoxb-12345...",
    get_all_activities: () => FIXTURE_ACTIVITIES,
    clear_cache: () => null,
    trigger_sync: () => "sync complete",
    get_config: () => FIXTURE_CONFIG,
    update_config: () => null,
    get_llm_summary: () => "**Summary:** You had a productive day with 2 PRs merged and 3 issues progressed.",
    get_chart_data: () => FIXTURE_CHART_DATA,
    get_feature_breakdown: () => FIXTURE_FEATURES,
    get_standup: () => "## What I Did\n- Merged PR: add dashboard grid layout\n- Completed issue: Fix settings modal styling\n\n## What I'm Working On\n- PR #101: add dark mode toggle (needs review)\n- REC-42: Implement heatmap drill-down (urgent)",
    get_open_tickets: () => FIXTURE_OPEN_TICKETS,
    get_open_prs: () => FIXTURE_OPEN_PRS,
    get_github_issues: () => FIXTURE_GITHUB_ISSUES,
    get_trends_data: () => FIXTURE_TRENDS_DATA,
    get_trends_ai_summary: () => "Your velocity is trending upward with a 15% increase in PRs merged this week.",
    get_heatmap_activities: () => [FIXTURE_ACTIVITIES[0], FIXTURE_ACTIVITIES[3]],
  };

  async function mockInvoke(cmd, args) {
    // Small delay to simulate async IPC
    await new Promise((r) => setTimeout(r, 5));

    const handler = COMMANDS[cmd];
    if (!handler) {
      console.warn(`[tauri-mock] Unknown command: ${cmd}`, args);
      return null;
    }
    const result = typeof handler === "function" ? handler(args) : handler;
    return JSON.parse(JSON.stringify(result)); // deep clone to prevent mutation
  }

  // -----------------------------------------------------------------------
  // Expose the mock under window.__TAURI__
  // -----------------------------------------------------------------------

  window.__TAURI__ = {
    core: {
      invoke: mockInvoke,
    },
    // Stubs for other Tauri APIs used by the app
    updater: {
      check: async () => null, // no update available
    },
    process: {
      relaunch: async () => {},
    },
    opener: {
      openUrl: (url) => {
        console.log("[tauri-mock] openUrl:", url);
      },
    },
  };

  // Make fixtures accessible from test code via window.__FIXTURES__
  window.__FIXTURES__ = {
    AUTH_STATUS: FIXTURE_AUTH_STATUS,
    CONFIG: FIXTURE_CONFIG,
    DIGEST: FIXTURE_DIGEST,
    ACTIVITIES: FIXTURE_ACTIVITIES,
    CHART_DATA: FIXTURE_CHART_DATA,
    FEATURES: FIXTURE_FEATURES,
    OPEN_PRS: FIXTURE_OPEN_PRS,
    OPEN_TICKETS: FIXTURE_OPEN_TICKETS,
    GITHUB_ISSUES: FIXTURE_GITHUB_ISSUES,
    TRENDS_DATA: FIXTURE_TRENDS_DATA,
    COMMANDS,
  };
})();
