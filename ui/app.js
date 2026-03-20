// ===== Tauri IPC Helper =====

async function invoke(cmd, args = {}) {
  try {
    return await window.__TAURI__.core.invoke(cmd, args);
  } catch (err) {
    console.error(`invoke(${cmd}) failed:`, err);
    throw err;
  }
}

// ===== State =====

const state = {
  period: "day",
  date: null,
  activeView: "overview",
  config: null,
  authStatus: { github: false, linear: false, slack: false, notion: false, anthropic: false },
  digest: null,
  standupText: null,
  briefingText: null,
  llmCache: {},
  standupCache: {},
  charts: {
    activity: null,
    pr: null,
  },
};

// ===== Constants =====

const SOURCE_META = {
  github: {
    label: "GitHub",
    color: "#238636",
    helper: 'Or run: <code>gh auth login</code> and Recap will use it automatically.',
    placeholder: "ghp_...",
  },
  linear: {
    label: "Linear",
    color: "#5e6ad2",
    helper: 'Get key: <a href="https://linear.app/settings/api" target="_blank">Settings &rarr; API</a>',
    placeholder: "lin_api_...",
  },
  // Slack and Notion disabled — Slack requires full OAuth, Notion requires app registration
  // slack: { ... },
  // notion: { ... },
};

const KIND_LABELS = {
  commit_pushed: "Commit",
  pr_opened: "PR Opened",
  pr_merged: "PR Merged",
  pr_reviewed: "Review",
  issue_opened: "Issue",
  issue_created: "Issue",
  issue_completed: "Done",
  issue_commented: "Comment",
  issue_prioritized: "Priority",
  issue_updated: "Updated",
  message_sent: "Message",
  thread_replied: "Reply",
  reaction_added: "Reaction",
  page_created: "Page",
  page_edited: "Edited",
  database_updated: "DB Update",
};

const KIND_CLASSES = {
  commit_pushed: "kind-commit",
  pr_opened: "kind-open",
  pr_merged: "kind-merged",
  pr_reviewed: "kind-review",
  issue_opened: "kind-issue",
  issue_created: "kind-issue",
  issue_completed: "kind-issue",
  issue_commented: "kind-comment",
  issue_prioritized: "kind-issue",
  issue_updated: "kind-issue",
  message_sent: "kind-message",
  thread_replied: "kind-message",
  reaction_added: "kind-message",
  page_created: "kind-page",
  page_edited: "kind-page",
  database_updated: "kind-page",
};

// Read CSS vars at runtime so Chart.js colors stay in sync
function cssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

// Lazy-initialized after DOM ready
let KIND_COLORS = {};
function initKindColors() {
  KIND_COLORS = {
    merges: cssVar('--status-merged'),
    reviews: cssVar('--status-review'),
    commits: cssVar('--status-backlog'),
    issues: cssVar('--linear'),
    messages: cssVar('--slack'),
    open: cssVar('--status-open'),
    completed: cssVar('--status-completed'),
    closed: cssVar('--status-closed'),
  };
}

// ===== DOM References =====

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => document.querySelectorAll(sel);

let dom = {};

// ===== Initialization =====

document.addEventListener("DOMContentLoaded", async () => {
  dom = {
    headerDate: $("#header-date"),
    tabBar: $("#tab-bar"),
    syncBtn: $("#sync-btn"),
    syncLabel: $("#sync-label"),
    authBanner: $("#auth-banner"),
    authBannerText: $("#auth-banner-text"),
    authBannerBtn: $("#auth-banner-btn"),
    main: $("#main"),
    loading: $("#loading"),
    emptyState: $("#empty-state"),
    dashboard: $("#dashboard"),
    briefingContent: $("#briefing-content"),
    briefingRefresh: $("#briefing-refresh"),
    chartActivity: $("#chart-activity"),
    chartPr: $("#chart-pr"),
    prStatsCounts: $("#pr-stats-counts"),
    featuresContent: $("#features-content"),
    linearContent: $("#linear-content"),
    activityTbody: $("#activity-tbody"),
    activityCount: $("#activity-count"),
    toast: $("#toast"),

    // Nav tabs & action bar
    navTabs: $$("#nav-tabs .nav-tab"),
    actionStandup: $("#action-standup"),
    actionSettings: $("#action-settings"),
    actionBar: $("#action-bar"),

    // View sections
    viewOverview: $("#view-overview"),
    viewGithub: $("#view-github"),
    viewLinear: $("#view-linear"),
    viewSlack: $("#view-slack"),

    // Standup modal
    standupOverlay: $("#standup-overlay"),
    standupClose: $("#standup-close"),
    standupModalContent: $("#standup-modal-content"),
    standupCopy: $("#standup-copy"),

    // Settings modal
    settingsOverlay: $("#settings-overlay"),
    settingsClose: $("#settings-close"),
    settingsAuthList: $("#settings-auth-list"),
    settingsPrefs: $("#settings-prefs"),

    // GitHub view
    githubStats: $("#github-stats"),
    githubPrTbody: $("#github-pr-tbody"),
    githubPrCount: $("#github-pr-count"),
    githubCommitTbody: $("#github-commit-tbody"),
    githubCommitCount: $("#github-commit-count"),
    githubReviewTbody: $("#github-review-tbody"),
    githubReviewCount: $("#github-review-count"),

    // Linear view
    linearStats: $("#linear-stats"),
    linearIssueTbody: $("#linear-issue-tbody"),
    linearIssueCount: $("#linear-issue-count"),

    // Slack view
    slackStats: $("#slack-stats"),
    slackChannels: $("#slack-channels"),
    slackMessageTbody: $("#slack-message-tbody"),
    slackMessageCount: $("#slack-message-count"),
  };

  initKindColors();
  renderDate();
  bindEvents();
  await refreshAuthStatus();
  await loadDashboard();
});

function bindEvents() {
  // Date navigation
  document.getElementById("date-prev")?.addEventListener("click", () => navigateDate(-1));
  document.getElementById("date-next")?.addEventListener("click", () => navigateDate(1));
  document.getElementById("date-today")?.addEventListener("click", goToToday);

  // Period tab switching
  dom.tabBar.addEventListener("click", (e) => {
    const tab = e.target.closest(".tab");
    if (!tab || tab.classList.contains("active")) return;
    $$(".tab").forEach((t) => t.classList.remove("active"));
    tab.classList.add("active");
    state.period = tab.dataset.period;
    loadDashboard().then(() => {
      if (state.activeView !== "overview") {
        renderSourceView(state.activeView);
      }
    });
  });

  // Nav tabs (view switching)
  dom.navTabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      dom.navTabs.forEach((t) => t.classList.remove("active"));
      tab.classList.add("active");
      switchView(tab.dataset.view);
    });
  });

  // Sync
  dom.syncBtn.addEventListener("click", handleSync);

  // Briefing refresh
  dom.briefingRefresh.addEventListener("click", () => {
    const key = cacheKey();
    delete state.llmCache[key];
    fetchBriefing();
  });

  // Action bar: standup
  dom.actionStandup.addEventListener("click", () => {
    openStandupModal();
  });

  // Action bar: settings
  dom.actionSettings.addEventListener("click", () => {
    openSettingsModal();
  });

  // Standup modal close
  dom.standupClose.addEventListener("click", () => {
    dom.standupOverlay.style.display = "none";
  });
  dom.standupOverlay.addEventListener("click", (e) => {
    if (e.target === dom.standupOverlay) {
      dom.standupOverlay.style.display = "none";
    }
  });

  // Standup copy
  dom.standupCopy.addEventListener("click", async () => {
    if (!state.standupText) {
      showToast("Nothing to copy yet.", "info");
      return;
    }
    try {
      await navigator.clipboard.writeText(state.standupText);
      showToast("Standup copied to clipboard!", "success");
    } catch {
      showToast("Failed to copy.", "error");
    }
  });

  // Settings modal close
  dom.settingsClose.addEventListener("click", () => {
    dom.settingsOverlay.style.display = "none";
  });
  dom.settingsOverlay.addEventListener("click", (e) => {
    if (e.target === dom.settingsOverlay) {
      dom.settingsOverlay.style.display = "none";
    }
  });

  // Auth banner opens settings modal
  dom.authBannerBtn.addEventListener("click", () => {
    openSettingsModal();
  });
}

// ===== View Switching =====

function switchView(view) {
  state.activeView = view;
  // Hide all view sections
  document.querySelectorAll('.view-section').forEach(s => s.style.display = 'none');
  // Show target
  const el = document.getElementById(`view-${view}`);
  if (el) el.style.display = '';
  // Load data
  loadViewData(view);
}

function loadViewData(view) {
  if (view === 'overview') {
    loadDashboard();
    return;
  }
  // Source views need digest data
  if (!state.digest) {
    loadDashboard().then(() => renderSourceView(view));
  } else {
    renderSourceView(view);
  }
}

function renderSourceView(view) {
  const activities = state.digest?.activities || [];
  if (view === 'github') renderGitHubView(activities.filter(a => a.source === 'github'));
  else if (view === 'linear') renderLinearView(activities.filter(a => a.source === 'linear'));
  else if (view === 'slack') renderSlackView(activities.filter(a => a.source === 'slack'));
}

// ===== Date Display =====

function renderDate() {
  const d = state.date ? new Date(state.date + "T00:00:00") : new Date();
  const opts = { weekday: "long", month: "long", day: "numeric", year: "numeric" };
  dom.headerDate.textContent = d.toLocaleDateString("en-US", opts);
}

function navigateDate(direction) {
  const current = state.date ? new Date(state.date + "T00:00:00") : new Date();
  const days = state.period === "day" ? 1 : state.period === "week" ? 7 : 30;
  current.setDate(current.getDate() + (direction * days));
  state.date = current.toISOString().slice(0, 10);
  renderDate();
  loadDashboard();
  if (state.activeView !== "overview") renderSourceView(state.activeView);
}

function goToToday() {
  state.date = null;
  renderDate();
  loadDashboard();
  if (state.activeView !== "overview") renderSourceView(state.activeView);
}

// ===== Auth =====

async function refreshAuthStatus() {
  try {
    state.authStatus = await invoke("get_auth_status");
  } catch {
    state.authStatus = { github: false, linear: false, slack: false, notion: false, anthropic: false };
  }
  renderAuthBanner();
}

function renderAuthBanner() {
  const sources = ["github", "linear", "slack", "notion"];
  const disconnected = sources.filter((s) => !state.authStatus[s]);

  if (disconnected.length === 0) {
    dom.authBanner.style.display = "none";
    return;
  }

  dom.authBanner.style.display = "";
  const names = disconnected.map((s) => SOURCE_META[s]?.label || s).join(", ");
  dom.authBannerText.innerHTML =
    `&#x26a0; ${disconnected.length} service${disconnected.length > 1 ? "s" : ""} not connected (${names})`;
}

async function saveToken(source) {
  const input = $(`#token-${source}`);
  const token = input?.value?.trim();
  if (!token) {
    showToast("Please enter a token.", "error");
    return;
  }
  try {
    await invoke("save_token", { source, token });
    showToast(`${SOURCE_META[source]?.label || source} connected!`, "success");
    await refreshAuthStatus();
    renderSettingsConnections();
    await loadDashboard();
  } catch (err) {
    showToast(`Failed to save token: ${err}`, "error");
  }
}

async function saveRefreshToken(source) {
  const input = $(`#refresh-token-${source}`);
  const token = input?.value?.trim();
  if (!token) {
    showToast("Please enter a refresh token.", "error");
    return;
  }
  try {
    await invoke("save_slack_refresh_token", { token });
    showToast("Slack refresh token saved!", "success");
  } catch (err) {
    showToast(`Failed to save refresh token: ${err}`, "error");
  }
}

// ===== Dashboard Loading =====

function cacheKey() {
  return `${state.period}:${state.date || "now"}`;
}

function invokeArgs() {
  const args = { period: state.period };
  if (state.date) args.date = state.date;
  return args;
}

async function loadDashboard() {
  const hlEl = document.getElementById("headline-metrics");
  dom.loading.style.display = "";
  dom.dashboard.style.display = "none";
  if (hlEl) hlEl.style.display = "none";
  dom.emptyState.style.display = "none";

  try {
    const args = invokeArgs();
    const [digest, chartData, features] = await Promise.all([
      invoke("get_digest", args),
      invoke("get_chart_data", args).catch(() => null),
      invoke("get_feature_breakdown", args).catch(() => null),
    ]);

    state.digest = digest;
    dom.loading.style.display = "none";

    if (!digest || !digest.activities || digest.activities.length === 0) {
      dom.emptyState.style.display = "";
      dom.dashboard.style.display = "none";
      if (hlEl) hlEl.style.display = "none";
      return;
    }

    dom.dashboard.style.display = "";
    if (hlEl) hlEl.style.display = "";

    renderHeadlineMetrics(digest);
    renderActivityChart(chartData);
    renderPrStats(digest.stats);
    renderFeatures(features);
    renderLinearProgress(digest.activities);
    renderActivityTable(digest.activities);

    // Fetch LLM content async (don't block dashboard)
    fetchBriefing();
  } catch (err) {
    dom.loading.style.display = "none";
    dom.emptyState.style.display = "";
    showToast(`Failed to load dashboard: ${err}`, "error");
  }
}

// ===== Headline Metrics =====

function renderHeadlineMetrics(digest) {
  const stats = digest?.stats;
  if (!stats) return;

  const total = stats.total_activities || 0;
  const merged = stats.by_kind?.["pr_merged"] || 0;
  const reviewed = stats.by_kind?.["pr_reviewed"] || 0;
  const issuesCompleted = stats.by_kind?.["issue_completed"] || 0;

  const set = (id, value) => {
    const el = document.querySelector(`#${id} .headline-value`);
    if (el) el.textContent = value;
  };

  set("hl-total", total);
  set("hl-prs", merged);
  set("hl-reviews", reviewed);
  set("hl-issues", issuesCompleted);
}

// ===== Activity Over Time Chart =====

function renderActivityChart(chartData) {
  if (!chartData || !chartData.labels || !chartData.datasets) {
    dom.chartActivity.parentElement.innerHTML = '<div class="no-data">No chart data available</div>';
    return;
  }

  if (!hasChartJs()) {
    dom.chartActivity.parentElement.innerHTML = renderFallbackStats(chartData);
    return;
  }

  if (state.charts.activity) {
    state.charts.activity.destroy();
  }

  const ctx = dom.chartActivity.getContext("2d");
  state.charts.activity = new Chart(ctx, {
    type: "bar",
    data: {
      labels: chartData.labels,
      datasets: [
        {
          label: "Merges",
          data: chartData.datasets.merges || [],
          backgroundColor: KIND_COLORS.merges,
          borderRadius: 3,
        },
        {
          label: "Reviews",
          data: chartData.datasets.reviews || [],
          backgroundColor: KIND_COLORS.reviews,
          borderRadius: 3,
        },
        {
          label: "Commits",
          data: chartData.datasets.commits || [],
          backgroundColor: KIND_COLORS.commits,
          borderRadius: 3,
        },
        {
          label: "Issues",
          data: chartData.datasets.issues || [],
          backgroundColor: KIND_COLORS.issues,
          borderRadius: 3,
        },
        {
          label: "Messages",
          data: chartData.datasets.messages || [],
          backgroundColor: KIND_COLORS.messages,
          borderRadius: 3,
        },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {
          position: "bottom",
          labels: {
            color: "#8a8a9a",
            font: { size: 11 },
            boxWidth: 12,
            padding: 12,
          },
        },
      },
      scales: {
        x: {
          stacked: true,
          ticks: { color: "#8a8a9a", font: { size: 11 } },
          grid: { display: false },
        },
        y: {
          stacked: true,
          ticks: { color: "#8a8a9a", font: { size: 11 }, stepSize: 1 },
          grid: { color: "rgba(255,255,255,0.04)" },
        },
      },
    },
  });
}

function renderFallbackStats(chartData) {
  const ds = chartData.datasets;
  const sum = (arr) => (arr || []).reduce((a, b) => a + b, 0);
  return `<div style="padding:10px;font-size:13px;color:var(--text-dim);">
    Merges: ${sum(ds.merges)} | Reviews: ${sum(ds.reviews)} | Commits: ${sum(ds.commits)} |
    Issues: ${sum(ds.issues)} | Messages: ${sum(ds.messages)}
  </div>`;
}

// ===== PR Stats (Donut) =====

function renderPrStats(stats) {
  if (!stats || !stats.by_kind) {
    dom.prStatsCounts.innerHTML = '<div class="no-data">No PR data</div>';
    return;
  }

  const opened = (stats.by_kind["pr_opened"] || 0);
  const merged = (stats.by_kind["pr_merged"] || 0);
  const reviewed = (stats.by_kind["pr_reviewed"] || 0);
  const total = opened + merged + reviewed;

  // Counts
  dom.prStatsCounts.innerHTML = `
    <div class="pr-stat-row">
      <span class="pr-stat-dot" style="background:${KIND_COLORS.open}"></span>
      <span class="pr-stat-label">Opened</span>
      <span class="pr-stat-value">${opened}</span>
    </div>
    <div class="pr-stat-row">
      <span class="pr-stat-dot" style="background:${KIND_COLORS.merges}"></span>
      <span class="pr-stat-label">Merged</span>
      <span class="pr-stat-value">${merged}</span>
    </div>
    <div class="pr-stat-row">
      <span class="pr-stat-dot" style="background:${KIND_COLORS.reviews}"></span>
      <span class="pr-stat-label">Reviewed</span>
      <span class="pr-stat-value">${reviewed}</span>
    </div>
  `;

  if (total === 0 || !hasChartJs()) return;

  if (state.charts.pr) {
    state.charts.pr.destroy();
  }

  const ctx = dom.chartPr.getContext("2d");
  state.charts.pr = new Chart(ctx, {
    type: "doughnut",
    data: {
      labels: ["Opened", "Merged", "Reviewed"],
      datasets: [
        {
          data: [opened, merged, reviewed],
          backgroundColor: [KIND_COLORS.open, KIND_COLORS.merges, KIND_COLORS.reviews],
          borderWidth: 0,
          hoverOffset: 4,
        },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      cutout: "65%",
      plugins: {
        legend: { display: false },
      },
    },
  });
}

// ===== Feature Areas =====

function renderFeatures(features) {
  if (!features || features.length === 0) {
    dom.featuresContent.innerHTML = '<div class="no-data">No feature data</div>';
    return;
  }

  // Sort by count descending, take top 8
  const sorted = [...features].sort((a, b) => b.count - a.count).slice(0, 8);
  const maxCount = sorted[0]?.count || 1;

  const kindColorMap = {
    commit_pushed: KIND_COLORS.commits,
    pr_opened: KIND_COLORS.open,
    pr_merged: KIND_COLORS.merges,
    pr_reviewed: KIND_COLORS.reviews,
    issue_opened: KIND_COLORS.issues,
    issue_created: KIND_COLORS.issues,
    issue_completed: KIND_COLORS.completed,
    issue_commented: KIND_COLORS.issues,
    issue_prioritized: KIND_COLORS.issues,
    issue_updated: KIND_COLORS.issues,
    message_sent: KIND_COLORS.messages,
    thread_replied: KIND_COLORS.messages,
    reaction_added: KIND_COLORS.messages,
    page_created: KIND_COLORS.commits,
    page_edited: KIND_COLORS.commits,
    database_updated: KIND_COLORS.commits,
  };

  let html = "";
  for (const f of sorted) {
    const pct = Math.round((f.count / maxCount) * 100);
    const name = escapeHtml(f.project || "Unknown");

    // Build segmented bar from kinds
    let segments = "";
    if (f.kinds && typeof f.kinds === "object") {
      const entries = Object.entries(f.kinds).sort((a, b) => b[1] - a[1]);
      for (const [kind, count] of entries) {
        const segPct = (count / f.count) * pct;
        const color = kindColorMap[kind] || KIND_COLORS.commits;
        segments += `<div class="feature-bar-segment" style="width:${segPct}%;background:${color}" title="${escapeAttr(kind)}: ${count}"></div>`;
      }
    } else {
      segments = `<div class="feature-bar-segment" style="width:${pct}%;background:var(--accent)"></div>`;
    }

    html += `
      <div class="feature-row">
        <div class="feature-label">
          <span>${name}</span>
          <span class="feature-label-count">${f.count}</span>
        </div>
        <div class="feature-bar-track">${segments}</div>
      </div>`;
  }

  dom.featuresContent.innerHTML = html;
}

// ===== Linear Progress =====

function renderLinearProgress(activities) {
  if (!activities || activities.length === 0) {
    dom.linearContent.innerHTML = '<div class="no-data">No Linear data</div>';
    return;
  }

  // Filter to Linear issues only
  const linearActivities = activities.filter((a) => a.source === "linear");

  if (linearActivities.length === 0) {
    dom.linearContent.innerHTML = '<div class="no-data">No Linear issues</div>';
    return;
  }

  let completed = 0;
  let started = 0;
  let other = 0;

  for (const a of linearActivities) {
    const kind = a.kind || "";
    if (kind === "issue_completed") {
      completed++;
    } else if (kind === "issue_created" || kind === "issue_opened" || kind === "issue_updated") {
      started++;
    } else {
      other++;
    }
  }

  const total = completed + started + other;
  if (total === 0) {
    dom.linearContent.innerHTML = '<div class="no-data">No Linear data</div>';
    return;
  }

  const rows = [
    { label: "Completed", count: completed, cls: "completed" },
    { label: "In Progress", count: started, cls: "started" },
    { label: "Other", count: other, cls: "backlog" },
  ];

  let html = "";
  for (const row of rows) {
    if (row.count === 0) continue;
    const pct = Math.max(Math.round((row.count / total) * 100), 4);
    html += `
      <div class="linear-stat-row">
        <span class="linear-stat-label">${row.label}</span>
        <div class="linear-stat-bar-track">
          <div class="linear-stat-bar ${row.cls}" style="width:${pct}%">${row.count}</div>
        </div>
      </div>`;
  }

  dom.linearContent.innerHTML = html;
}

// ===== Activity Table =====

function renderActivityTable(activities) {
  if (!activities || activities.length === 0) {
    dom.activityTbody.innerHTML = '<tr><td colspan="5" class="no-data">No activities</td></tr>';
    dom.activityCount.textContent = "";
    return;
  }

  // Sort descending by occurred_at
  const sorted = [...activities].sort(
    (a, b) => new Date(b.occurred_at) - new Date(a.occurred_at)
  );

  dom.activityCount.textContent = `${sorted.length} activit${sorted.length === 1 ? "y" : "ies"}`;

  let html = "";
  for (const a of sorted) {
    const kind = (a.kind || "").toLowerCase();
    const kindClass = KIND_CLASSES[kind] || "kind-default";
    const kindLabel = KIND_LABELS[kind] || formatKind(kind);
    const source = (a.source || "").toLowerCase();
    const sourceLabel = SOURCE_META[source]?.label || source;
    const relTime = relativeTime(a.occurred_at);
    const title = escapeHtml(a.title || "");
    const project = a.project ? `<span class="project-tag">${escapeHtml(a.project)}</span>` : "&mdash;";

    const titleHtml = a.url
      ? `<a class="activity-title-link" href="${escapeAttr(a.url)}" target="_blank" title="${escapeAttr(a.title)}">${title}</a>`
      : `<span class="activity-title-text" title="${escapeAttr(a.title)}">${title}</span>`;

    html += `<tr>
      <td><span class="kind-badge ${kindClass}">${kindLabel}</span></td>
      <td style="max-width:400px">${titleHtml}</td>
      <td>${project}</td>
      <td><span class="source-badge ${source}">${sourceLabel}</span></td>
      <td><span class="time-dim">${relTime}</span></td>
    </tr>`;
  }

  dom.activityTbody.innerHTML = html;
}

function formatKind(kind) {
  if (!kind) return "?";
  return kind.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

// ===== GitHub View =====

function renderGitHubView(activities) {
  // Store on state for filter callbacks
  state._ghPrs = activities.filter(a => ['pr_opened', 'pr_merged'].includes(a.kind));
  state._ghCommits = activities.filter(a => a.kind === 'commit_pushed');
  state._ghReviews = activities.filter(a => a.kind === 'pr_reviewed');

  const prs = state._ghPrs;
  const commits = state._ghCommits;
  const reviews = state._ghReviews;
  const merged = prs.filter(p => p.kind === 'pr_merged').length;
  const opened = prs.filter(p => p.kind === 'pr_opened').length;

  // Stats row
  dom.githubStats.innerHTML = renderStatCards([
    { value: prs.length, label: 'Pull Requests' },
    { value: merged, label: 'Merged' },
    { value: reviews.length, label: 'Reviews' },
    { value: commits.length, label: 'Commits' },
  ]);

  // Status + CC combined filter bar
  const ccFilterEl = document.getElementById('github-cc-filters');
  if (ccFilterEl) {
    // Status filters
    const statusFilters = [
      { key: 'all', label: 'All', count: prs.length, cls: '' },
      { key: 'merged', label: 'Merged', count: merged, cls: 'kind-merged' },
      { key: 'open', label: 'Open', count: opened, cls: 'kind-open' },
    ];

    // CC type filters
    const ccCounts = {};
    for (const pr of prs) {
      const cc = pr.metadata?.cc_type;
      if (cc) ccCounts[cc] = (ccCounts[cc] || 0) + 1;
    }
    const ccTypes = Object.entries(ccCounts).sort((a, b) => b[1] - a[1]);

    ccFilterEl.innerHTML =
      statusFilters.map(f =>
        `<button class="cc-filter-btn ${f.key === 'all' ? 'active' : ''}" data-status="${f.key}">${f.cls ? `<span class="kind-badge ${f.cls}" style="font-size:9px;padding:1px 5px">${f.label}</span>` : f.label} ${f.count}</button>`
      ).join('') +
      (ccTypes.length ? '<span style="border-left:1px solid var(--border);margin:0 4px;height:20px;display:inline-block;vertical-align:middle"></span>' : '') +
      ccTypes.map(([type, count]) =>
        `<button class="cc-filter-btn" data-cc="${escapeAttr(type)}"><span class="cc-tag cc-${type}">${type}</span> ${count}</button>`
      ).join('');

    ccFilterEl.querySelectorAll('.cc-filter-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        ccFilterEl.querySelectorAll('.cc-filter-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        const status = btn.dataset.status;
        const cc = btn.dataset.cc;
        if (status) renderPrTable(prs, 'all', status);
        else if (cc) renderPrTable(prs, cc, 'all');
      });
    });
  }

  renderPrTable(prs, 'all', 'all');

  // Commits table
  dom.githubCommitCount.textContent = `${commits.length} commits`;
  dom.githubCommitTbody.innerHTML = commits.sort((a, b) => new Date(b.occurred_at) - new Date(a.occurred_at)).map(c => `<tr>
    <td style="max-width:400px"><span class="activity-title-text">${escapeHtml(c.title)}</span></td>
    <td>${c.project ? `<span class="project-tag">${escapeHtml(c.project)}</span>` : '—'}</td>
    <td><span class="time-dim">${relativeTime(c.occurred_at)}</span></td>
  </tr>`).join('');

  // Reviews table
  dom.githubReviewCount.textContent = `${reviews.length} reviews`;
  dom.githubReviewTbody.innerHTML = reviews.sort((a, b) => new Date(b.occurred_at) - new Date(a.occurred_at)).map(r => `<tr>
    <td style="max-width:400px">${r.url ? `<a class="activity-title-link" href="${escapeAttr(r.url)}" target="_blank">${escapeHtml(r.title)}</a>` : escapeHtml(r.title)}</td>
    <td>${r.project ? `<span class="project-tag">${escapeHtml(r.project)}</span>` : '—'}</td>
    <td><span class="time-dim">${relativeTime(r.occurred_at)}</span></td>
  </tr>`).join('');
}

function renderPrTable(prs, ccFilter, statusFilter) {
  let filtered = prs;
  if (statusFilter && statusFilter !== 'all') {
    filtered = filtered.filter(pr => {
      if (statusFilter === 'merged') return pr.kind === 'pr_merged';
      if (statusFilter === 'open') return pr.kind === 'pr_opened';
      return true;
    });
  }
  if (ccFilter && ccFilter !== 'all') {
    filtered = filtered.filter(pr => (pr.metadata?.cc_type || 'other') === ccFilter);
  }

  // Deduplicate by URL
  const prMap = new Map();
  for (const pr of filtered.sort((a, b) => new Date(a.occurred_at) - new Date(b.occurred_at))) {
    prMap.set(pr.url || pr.source_id, pr);
  }
  const uniquePrs = [...prMap.values()].sort((a, b) => new Date(b.occurred_at) - new Date(a.occurred_at));

  dom.githubPrCount.textContent = `${uniquePrs.length} PRs`;
  dom.githubPrTbody.innerHTML = uniquePrs.map(pr => {
    const cc = pr.metadata?.cc_type;
    const ccScope = pr.metadata?.cc_scope;
    const ccHtml = cc ? `<span class="cc-tag cc-${cc}">${cc}${ccScope ? `(${escapeHtml(ccScope)})` : ''}</span>` : '';
    const isMerged = pr.kind === 'pr_merged';
    const statusClass = isMerged ? 'kind-merged' : 'kind-open';
    const statusLabel = isMerged ? 'Merged' : 'Open';

    return `<tr>
      <td><span class="kind-badge ${statusClass}">${statusLabel}</span></td>
      <td>${ccHtml || '<span style="color:var(--text-muted)">—</span>'}</td>
      <td style="max-width:400px">${pr.url ? `<a class="activity-title-link" href="${escapeAttr(pr.url)}" target="_blank">${escapeHtml(pr.title)}</a>` : `<span class="activity-title-text">${escapeHtml(pr.title)}</span>`}</td>
      <td>${pr.project ? `<span class="project-tag">${escapeHtml(pr.project)}</span>` : '—'}</td>
      <td><span class="time-dim">${relativeTime(pr.occurred_at)}</span></td>
    </tr>`;
  }).join('');
}

// ===== Linear View =====

function renderLinearView(activities) {
  state._linearActivities = activities;

  const completed = activities.filter(a => a.kind === 'issue_completed');
  const inProgress = activities.filter(a => ['issue_created', 'issue_updated', 'issue_opened'].includes(a.kind));
  const other = activities.filter(a => !['issue_completed', 'issue_created', 'issue_updated', 'issue_opened'].includes(a.kind));

  dom.linearStats.innerHTML = renderStatCards([
    { value: completed.length, label: 'Completed' },
    { value: inProgress.length, label: 'In Progress' },
    { value: activities.length, label: 'Total' },
  ]);

  // Filter bar
  const filterEl = document.getElementById('linear-filters');
  if (filterEl) {
    const filters = [
      { key: 'all', label: 'All', count: activities.length },
      { key: 'completed', label: 'Completed', count: completed.length, cls: 'state-completed' },
      { key: 'started', label: 'In Progress', count: inProgress.length, cls: 'state-started' },
      { key: 'other', label: 'Other', count: other.length, cls: 'state-backlog' },
    ];
    filterEl.innerHTML = filters.map(f =>
      `<button class="cc-filter-btn ${f.key === 'all' ? 'active' : ''}" data-linear-filter="${f.key}">${f.cls ? `<span class="state-badge ${f.cls}" style="font-size:10px">${f.label}</span>` : f.label} ${f.count}</button>`
    ).join('');
    filterEl.querySelectorAll('.cc-filter-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        filterEl.querySelectorAll('.cc-filter-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        renderLinearTable(state._linearActivities, btn.dataset.linearFilter);
      });
    });
  }

  renderLinearTable(activities, 'all');
}

function renderLinearTable(activities, filter) {
  const completed = activities.filter(a => a.kind === 'issue_completed');
  const inProgress = activities.filter(a => ['issue_created', 'issue_updated', 'issue_opened'].includes(a.kind));
  const other = activities.filter(a => !['issue_completed', 'issue_created', 'issue_updated', 'issue_opened'].includes(a.kind));

  let sorted;
  if (filter === 'completed') sorted = completed;
  else if (filter === 'started') sorted = inProgress;
  else if (filter === 'other') sorted = other;
  else sorted = [...completed, ...inProgress, ...other];

  dom.linearIssueCount.textContent = `${sorted.length} issues`;

  dom.linearIssueTbody.innerHTML = sorted.map(a => {
    const stateType = a.metadata?.state_type || '';
    const stateName = a.metadata?.state || a.kind.replace('issue_', '');
    const stateClass = stateType === 'completed' ? 'state-completed' : stateType === 'started' ? 'state-started' : stateType === 'cancelled' ? 'state-cancelled' : 'state-backlog';
    const identifier = a.metadata?.issue_identifier || a.source_id || '';
    const priorityLabel = a.metadata?.priority_label || '';
    const priorityClass = priorityLabel.toLowerCase().includes('urgent') ? 'urgent' : priorityLabel.toLowerCase().includes('high') ? 'high' : priorityLabel.toLowerCase().includes('medium') ? 'medium' : 'low';

    return `<tr>
      <td>${a.url ? `<a href="${escapeAttr(a.url)}" target="_blank" class="kind-badge kind-issue" style="text-decoration:none">${escapeHtml(identifier)}</a>` : `<span class="kind-badge kind-issue">${escapeHtml(identifier)}</span>`}</td>
      <td style="max-width:300px">${a.url ? `<a class="activity-title-link" href="${escapeAttr(a.url)}" target="_blank">${escapeHtml(a.title)}</a>` : escapeHtml(a.title)}</td>
      <td><span class="state-badge ${stateClass}">${escapeHtml(stateName)}</span></td>
      <td>${priorityLabel ? `<span class="priority-badge ${priorityClass}">${escapeHtml(priorityLabel)}</span>` : '—'}</td>
      <td>${a.project ? escapeHtml(a.project) : '—'}</td>
      <td><span class="time-dim">${relativeTime(a.occurred_at)}</span></td>
    </tr>`;
  }).join('');
}

// ===== Slack View =====

function renderSlackView(activities) {
  // Group by channel (project field)
  const channels = {};
  for (const a of activities) {
    const ch = a.project || 'DM';
    if (!channels[ch]) channels[ch] = { messages: 0, threads: 0 };
    if (a.kind === 'thread_replied') channels[ch].threads++;
    else channels[ch].messages++;
  }

  dom.slackStats.innerHTML = renderStatCards([
    { value: activities.length, label: 'Total Messages' },
    { value: Object.keys(channels).length, label: 'Channels' },
    { value: activities.filter(a => a.kind === 'thread_replied').length, label: 'Thread Replies' },
  ]);

  // Channel cards
  const sortedChannels = Object.entries(channels).sort((a, b) => (b[1].messages + b[1].threads) - (a[1].messages + a[1].threads));
  dom.slackChannels.innerHTML = sortedChannels.map(([name, stats]) => `
    <div class="slack-channel-card">
      <div class="slack-channel-name">#${escapeHtml(name)}</div>
      <div class="slack-channel-stat">${stats.messages} messages, ${stats.threads} replies</div>
    </div>
  `).join('');

  // Message table
  const sorted = [...activities].sort((a, b) => new Date(b.occurred_at) - new Date(a.occurred_at));
  dom.slackMessageCount.textContent = `${sorted.length} messages`;
  dom.slackMessageTbody.innerHTML = sorted.map(a => `<tr>
    <td><span class="project-tag">#${escapeHtml(a.project || 'DM')}</span></td>
    <td style="max-width:400px">${a.url ? `<a class="activity-title-link" href="${escapeAttr(a.url)}" target="_blank">${escapeHtml(a.description || a.title)}</a>` : `<span class="activity-title-text">${escapeHtml(a.description || a.title)}</span>`}</td>
    <td><span class="kind-badge kind-message">${a.kind === 'thread_replied' ? 'Reply' : 'Message'}</span></td>
    <td><span class="time-dim">${relativeTime(a.occurred_at)}</span></td>
  </tr>`).join('');
}

// ===== Stat Cards Helper =====

function renderStatCards(stats) {
  return stats.map(s => `
    <div class="source-stat-card">
      <div class="source-stat-value">${s.value}</div>
      <div class="source-stat-label">${s.label}</div>
    </div>
  `).join('');
}

// ===== Daily Briefing (LLM) =====

async function fetchBriefing() {
  const key = cacheKey();

  if (state.llmCache[key] !== undefined) {
    renderBriefing(state.llmCache[key]);
    return;
  }

  // Loading dots
  dom.briefingContent.innerHTML = loadingDotsHtml();

  try {
    const summary = await invoke("get_llm_summary", invokeArgs());
    state.llmCache[key] = summary;
    state.briefingText = summary;
    renderBriefing(summary);
  } catch {
    renderBriefing(null);
  }
}

function renderBriefing(summary) {
  if (summary) {
    dom.briefingContent.innerHTML = renderMarkdown(summary);
  } else {
    dom.briefingContent.innerHTML =
      '<span class="briefing-disabled">Enable LLM summaries in <code>~/.config/recap/config.toml</code> with <code>llm.enabled = true</code> and an Anthropic API key.</span>';
  }
}

// ===== Standup Modal =====

async function openStandupModal() {
  dom.standupOverlay.style.display = '';
  const key = cacheKey();
  if (state.standupCache[key]) {
    state.standupText = state.standupCache[key];
    dom.standupModalContent.innerHTML = renderMarkdown(state.standupText);
    return;
  }
  dom.standupModalContent.innerHTML = loadingDotsHtml();
  try {
    const args = {};
    if (state.date) args.date = state.date;
    const standup = await invoke('get_standup', args);
    state.standupCache[key] = standup;
    state.standupText = standup;
    dom.standupModalContent.innerHTML = standup ? renderMarkdown(standup) : '<span class="briefing-disabled">Could not generate standup. Is Claude CLI installed?</span>';
  } catch (err) {
    dom.standupModalContent.innerHTML = `<span class="briefing-disabled">Error: ${escapeHtml(String(err))}</span>`;
  }
}

function loadingDotsHtml() {
  return '<div class="loading-dots"><span class="pulse-dot"></span><span class="pulse-dot"></span><span class="pulse-dot"></span></div>';
}

// ===== Settings Modal =====

async function openSettingsModal() {
  dom.settingsOverlay.style.display = '';
  try {
    state.config = await invoke('get_config');
  } catch {}
  await refreshAuthStatus();
  renderSettingsConnections();
  renderSettingsPrefs();
}

function renderSettingsConnections() {
  let html = '';
  for (const [source, meta] of Object.entries(SOURCE_META)) {
    const connected = state.authStatus[source];
    html += `<div class="auth-source">
      <div class="auth-dot ${connected ? 'connected' : 'disconnected'}"></div>
      <div class="auth-info">
        <div class="auth-name">${meta.label}</div>
        ${connected
          ? '<div class="auth-helper" style="color:#34d058;">Connected</div>'
          : `<div class="auth-helper">${meta.helper}</div>
             <div class="auth-input-row">
               <input type="password" class="auth-input" id="token-${source}" placeholder="${escapeAttr(meta.placeholder)}" autocomplete="off" spellcheck="false">
               <button class="btn btn-small btn-highlight" data-save-token="${source}">Save</button>
             </div>
             ${meta.hasTokenExchange ? `
             <div style="margin-top:8px;font-size:11px;color:var(--text-dim);font-weight:600;">Or connect with refresh token:</div>
             <div class="auth-input-row" style="margin-top:4px">
               <input type="password" class="auth-input" id="slack-refresh-token" placeholder="xoxe-1-... (refresh token)" autocomplete="off" spellcheck="false">
             </div>
             <div class="auth-input-row" style="margin-top:4px">
               <input type="text" class="auth-input" id="slack-client-id" placeholder="Client ID (from Slack app settings)" autocomplete="off" spellcheck="false">
             </div>
             <div class="auth-input-row" style="margin-top:4px">
               <input type="password" class="auth-input" id="slack-client-secret" placeholder="Client Secret" autocomplete="off" spellcheck="false">
               <button class="btn btn-small btn-highlight" id="slack-exchange-btn">Connect</button>
             </div>` : ''}`
        }
      </div>
    </div>`;
  }
  dom.settingsAuthList.innerHTML = html;
  // Bind save buttons
  dom.settingsAuthList.querySelectorAll('[data-save-token]').forEach(btn => {
    btn.addEventListener('click', () => saveToken(btn.dataset.saveToken));
  });
  const exchangeBtn = dom.settingsAuthList.querySelector('#slack-exchange-btn');
  if (exchangeBtn) {
    exchangeBtn.addEventListener('click', exchangeSlackToken);
  }
}

async function exchangeSlackToken() {
  const refreshToken = document.getElementById('slack-refresh-token')?.value?.trim();
  const clientId = document.getElementById('slack-client-id')?.value?.trim();
  const clientSecret = document.getElementById('slack-client-secret')?.value?.trim();

  if (!refreshToken || !clientId || !clientSecret) {
    showToast('All three fields are required.', 'error');
    return;
  }

  try {
    const msg = await invoke('exchange_slack_refresh_token', {
      refreshToken, clientId, clientSecret,
    });
    showToast(msg, 'success');
    await refreshAuthStatus();
    renderSettingsConnections();
  } catch (err) {
    showToast(`Slack exchange failed: ${err}`, 'error');
  }
}

function renderSettingsPrefs() {
  if (!state.config) { dom.settingsPrefs.innerHTML = ''; return; }
  const c = state.config;
  dom.settingsPrefs.innerHTML = `
    <div class="pref-row">
      <label class="pref-label">Sync interval (min)</label>
      <input class="pref-input" type="number" id="pref-sync-interval" value="${c.schedule?.sync_interval_minutes || 5}" min="1" max="60">
    </div>
    <div class="pref-row">
      <label class="pref-label">Daily reminder</label>
      <input class="pref-input" type="text" id="pref-reminder-time" value="${c.schedule?.daily_reminder_time || '17:00'}" placeholder="HH:MM">
    </div>
    <div class="pref-row">
      <label class="pref-label">GitHub username</label>
      <input class="pref-input" type="text" id="pref-gh-username" value="${escapeAttr(c.github?.username || '')}" placeholder="Auto-detected from gh CLI">
    </div>
    <div class="pref-row">
      <label class="pref-label">Slack user ID</label>
      <input class="pref-input" type="text" id="pref-slack-userid" value="${escapeAttr(c.slack?.user_id || '')}" placeholder="U...">
    </div>
    <div class="pref-row">
      <label class="pref-label">Ignored channels</label>
      <input class="pref-input" type="text" id="pref-ignored-channels" value="${escapeAttr((c.slack?.ignored_channels || []).join(', '))}" placeholder="github-prs, graphite-*">
      <div class="pref-hint">Comma-separated, supports glob patterns</div>
    </div>
    <div class="pref-save-row">
      <button class="btn btn-highlight" id="pref-save-btn">Save Preferences</button>
    </div>
    <div style="margin-top:20px;padding-top:16px;border-top:1px solid var(--border)">
      <div style="font-size:12px;color:var(--text-dim);margin-bottom:8px">Clear all synced activities, LLM summaries, and sync cursors. Tokens are kept.</div>
      <button class="btn" id="pref-clear-cache-btn" style="background:var(--highlight)">Clear Cached Data</button>
    </div>
  `;
  document.getElementById('pref-save-btn')?.addEventListener('click', savePreferences);
  const clearBtn = document.getElementById('pref-clear-cache-btn');
  if (clearBtn) {
    clearBtn.addEventListener('click', async () => {
      console.log('clear cache button clicked');
      clearBtn.disabled = true;
      clearBtn.textContent = 'Clearing...';
      try {
        await invoke('clear_cache');
        state.digest = null;
        state.llmCache = {};
        state.standupCache = {};
        clearBtn.textContent = 'Cleared!';
        showToast('Cache cleared! Hit Sync to re-fetch.', 'success');
      } catch (err) {
        console.error('clear_cache failed:', err);
        clearBtn.textContent = 'Clear Cached Data';
        showToast(`Failed to clear cache: ${err}`, 'error');
      } finally {
        clearBtn.disabled = false;
        setTimeout(() => { clearBtn.textContent = 'Clear Cached Data'; }, 2000);
      }
    });
  } else {
    console.warn('pref-clear-cache-btn not found');
  }
}

async function savePreferences() {
  if (!state.config) return;
  state.config.schedule.sync_interval_minutes = parseInt(document.getElementById('pref-sync-interval')?.value) || 5;
  state.config.schedule.daily_reminder_time = document.getElementById('pref-reminder-time')?.value || '17:00';
  const ghUsername = document.getElementById('pref-gh-username')?.value?.trim();
  state.config.github.username = ghUsername || null;
  const slackUserId = document.getElementById('pref-slack-userid')?.value?.trim();
  state.config.slack.user_id = slackUserId || null;
  const ignoredStr = document.getElementById('pref-ignored-channels')?.value || '';
  state.config.slack.ignored_channels = ignoredStr.split(',').map(s => s.trim()).filter(Boolean);
  try {
    await invoke('update_config', { config: state.config });
    showToast('Preferences saved!', 'success');
  } catch (err) {
    showToast(`Failed to save: ${err}`, 'error');
  }
}

// ===== Sync =====

async function handleSync() {
  dom.syncBtn.disabled = true;
  dom.syncBtn.classList.add("syncing");
  dom.syncBtn.textContent = "Syncing...";

  try {
    const msg = await invoke("trigger_sync");
    showToast(msg || "Sync complete", "success");
    await refreshAuthStatus();
    await loadDashboard();
  } catch (err) {
    showToast(`Sync failed: ${err}`, "error");
  } finally {
    dom.syncBtn.disabled = false;
    dom.syncBtn.classList.remove("syncing");
    dom.syncBtn.textContent = "↻ Sync";
  }
}

// ===== Markdown Renderer =====

function renderMarkdown(text) {
  let html = escapeHtml(text);
  // **bold**
  html = html.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");
  // ### headings
  html = html.replace(/^### (.+)$/gm, "<h3>$1</h3>");
  html = html.replace(/^## (.+)$/gm, "<h2>$1</h2>");
  html = html.replace(/^# (.+)$/gm, "<h1>$1</h1>");
  // `inline code`
  html = html.replace(/`([^`]+)`/g, "<code>$1</code>");
  // - bullet items (consecutive lines)
  html = html.replace(/^- (.+)$/gm, "<li>$1</li>");
  html = html.replace(/((?:<li>.*<\/li>\n?)+)/g, "<ul>$1</ul>");
  // Numbered lists
  html = html.replace(/^\d+\. (.+)$/gm, "<li>$1</li>");
  return html;
}

// ===== Toast =====

let toastTimer = null;

function showToast(message, type = "info") {
  if (toastTimer) clearTimeout(toastTimer);
  dom.toast.textContent = message;
  dom.toast.className = `toast ${type}`;
  dom.toast.style.display = "";
  toastTimer = setTimeout(() => {
    dom.toast.style.display = "none";
  }, 3000);
}

// ===== Utilities =====

function hasChartJs() {
  return typeof Chart !== "undefined";
}

function relativeTime(dateStr) {
  if (!dateStr) return "";
  const now = Date.now();
  const then = new Date(dateStr).getTime();
  const diffMs = now - then;
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHr = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHr / 24);

  if (diffSec < 0) return "just now";
  if (diffSec < 60) return "just now";
  if (diffMin < 60) return `${diffMin}m ago`;
  if (diffHr < 24) return `${diffHr}h ago`;
  if (diffDay === 1) return "yesterday";
  if (diffDay < 7) return `${diffDay}d ago`;
  if (diffDay < 30) return `${Math.floor(diffDay / 7)}w ago`;
  return new Date(dateStr).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function escapeHtml(str) {
  if (!str) return "";
  const el = document.createElement("span");
  el.textContent = str;
  return el.innerHTML;
}

function escapeAttr(str) {
  if (!str) return "";
  return str
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
