// ===== Tauri IPC Helper =====

async function invoke(cmd, args = {}) {
  try {
    return await window.__TAURI__.core.invoke(cmd, args);
  } catch (err) {
    console.error(`invoke(${cmd}) failed:`, err);
    throw err;
  }
}

// ===== Auto-Update =====

// updateStatus: "checking" | "up_to_date" | "available" | "error"
async function checkForUpdates(silent = true) {
  state.updateStatus = 'checking';
  state.updateVersion = null;
  state.updateError = null;
  state._updateObj = null;

  if (!window.__TAURI__?.updater?.check) {
    console.error("Updater API not available");
    state.updateStatus = 'error';
    state.updateError = 'Updater API not available';
    renderUpdateBanner();
    return;
  }

  try {
    console.log("Checking for updates...");
    const { check } = window.__TAURI__.updater;
    const update = await check();
    if (update?.available) {
      console.log(`Update available: v${update.version}`);
      state.updateStatus = 'available';
      state.updateVersion = update.version;
      state._updateObj = update;
      if (!silent) {
        const shouldUpdate = confirm(
          `A new version (${update.version}) is available. Update now?`
        );
        if (shouldUpdate) {
          await update.downloadAndInstall();
          const { relaunch } = window.__TAURI__.process;
          await relaunch();
        }
      }
    } else {
      console.log("App is up to date");
      state.updateStatus = 'up_to_date';
    }
  } catch (e) {
    console.error("Update check failed:", e);
    state.updateStatus = 'error';
    state.updateError = String(e);
  }
  renderUpdateBanner();
}

async function installUpdate() {
  if (!state._updateObj) return;
  try {
    await state._updateObj.downloadAndInstall();
    const { relaunch } = window.__TAURI__.process;
    await relaunch();
  } catch (e) {
    showToast(`Update failed: ${e}`, 'error');
  }
}

// ===== Update Banner (main view) =====

function renderUpdateBanner() {
  const banner = document.getElementById('update-banner');
  if (!banner) return;

  const dot = banner.querySelector('.update-banner-dot');
  const installBtn = document.getElementById('update-banner-install');

  if (state.updateStatus === 'available' && state.updateVersion && !state.updateBannerDismissed) {
    document.getElementById('update-banner-text').textContent =
      `v${state.updateVersion} available`;
    banner.classList.remove('update-banner--error');
    if (dot) dot.style.background = '';
    if (installBtn) installBtn.style.display = '';
    banner.style.display = '';
  } else if (state.updateStatus === 'error' && !state.updateBannerDismissed) {
    document.getElementById('update-banner-text').textContent =
      'Update check failed \u2014 see Settings for details';
    banner.classList.add('update-banner--error');
    if (dot) dot.style.background = 'var(--highlight)';
    if (installBtn) installBtn.style.display = 'none';
    banner.style.display = '';
  } else {
    banner.classList.remove('update-banner--error');
    banner.style.display = 'none';
  }
}

function dismissUpdateBanner() {
  state.updateBannerDismissed = true;
  const banner = document.getElementById('update-banner');
  if (banner) banner.style.display = 'none';
}

function bindUpdateBannerEvents() {
  document.getElementById('update-banner-dismiss')?.addEventListener('click', dismissUpdateBanner);
  document.getElementById('update-banner-install')?.addEventListener('click', installUpdate);
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
    velocity: null,
    cycleTime: null,
    focus: null,
    burnout: null,
    forecast: null,
    productivity: null,
  },
  gridLayout: null,
  gridEditMode: false,
  lastSyncTime: null,
  updateBannerDismissed: false,
  trendsAdvanced: false,
};

// ===== Grid Layout System =====

const DEFAULT_LAYOUT = {
  "card-briefing":       { x: 1,  y: 1,  w: 6,  h: 5 },
  "card-activity-chart": { x: 7,  y: 1,  w: 6,  h: 4 },
  "card-pr-stats":       { x: 7,  y: 5,  w: 6,  h: 3 },
  "card-features":       { x: 1,  y: 6,  w: 6,  h: 3 },
  "card-linear":         { x: 1,  y: 9,  w: 12, h: 3 },
  "card-activity-table": { x: 1,  y: 12, w: 12, h: 5 },
};

let _gridDragState = null;
let _gridResizeState = null;
let _saveLayoutTimer = null;

function toggleEditMode() {
  state.gridEditMode = !state.gridEditMode;
  const main = document.getElementById("main");
  if (state.gridEditMode) {
    main.classList.add("grid-edit-mode");
    dom.actionEditLayout.classList.add("edit-active");
    dom.actionEditLayout.innerHTML = "&#x2714; Done Editing";
    dom.actionResetLayout.classList.remove("hidden");
  } else {
    main.classList.remove("grid-edit-mode");
    dom.actionEditLayout.classList.remove("edit-active");
    dom.actionEditLayout.innerHTML = "&#x270e; Edit Layout";
    dom.actionResetLayout.classList.add("hidden");
  }
}

function initGridSystem() {
  loadGridLayout();

  // Inject resize handles and drag icons into each grid card (once)
  const grid = document.getElementById("dashboard");
  if (!grid) return;
  const cards = grid.querySelectorAll(".card[id]");
  cards.forEach((card) => {
    if (!card.querySelector(".resize-handle")) {
      const handle = document.createElement("div");
      handle.className = "resize-handle";
      card.appendChild(handle);
    }

    const header = card.querySelector(".card-header");
    if (header && !header.querySelector(".drag-handle-icon")) {
      const icon = document.createElement("span");
      icon.className = "drag-handle-icon";
      icon.innerHTML = "&#x2630;"; // hamburger/grip icon
      header.prepend(icon);
    }

    // Drag: mousedown on card-header (only in edit mode)
    if (header && !header._gridDragBound) {
      header._gridDragBound = true;
      header.addEventListener("mousedown", (e) => {
        if (!state.gridEditMode) return;
        if (e.target.closest("button")) return;
        e.preventDefault();
        startDrag(e, card.id);
      });
    }

    // Resize: mousedown on handle (only in edit mode)
    const handle = card.querySelector(".resize-handle");
    if (handle && !handle._gridResizeBound) {
      handle._gridResizeBound = true;
      handle.addEventListener("mousedown", (e) => {
        if (!state.gridEditMode) return;
        e.preventDefault();
        e.stopPropagation();
        startResize(e, card.id);
      });
    }
  });

  applyLayout(state.gridLayout);
}

function applyLayout(layout) {
  if (!layout) return;
  for (const [cardId, pos] of Object.entries(layout)) {
    const card = document.getElementById(cardId);
    if (!card) continue;
    card.style.gridColumn = `${pos.x} / span ${pos.w}`;
    card.style.gridRow = `${pos.y} / span ${pos.h}`;
  }
}

function rectsOverlap(a, b) {
  // Grid coords: x is col start (1-based), w is span, same for y/h
  return (
    a.x < b.x + b.w &&
    a.x + a.w > b.x &&
    a.y < b.y + b.h &&
    a.y + a.h > b.y
  );
}

function resolveCollisions(movedCardId) {
  // Push any card that overlaps with movedCard downward, then cascade.
  // We iterate until no more overlaps remain (with a safety cap).
  const layout = state.gridLayout;
  const moved = layout[movedCardId];
  if (!moved) return;

  // Build a list of cards to check, sorted top-to-bottom by y
  const cardIds = Object.keys(layout).filter((id) => id !== movedCardId);

  // Phase 1: push anything overlapping the moved card
  for (const otherId of cardIds) {
    const other = layout[otherId];
    if (rectsOverlap(moved, other)) {
      other.y = moved.y + moved.h;
    }
  }

  // Phase 2: cascade — repeatedly resolve pairwise overlaps top-to-bottom
  // until stable (max 20 iterations to avoid infinite loops)
  for (let iter = 0; iter < 20; iter++) {
    let changed = false;
    // Sort all cards by y so we push downward
    const sorted = Object.keys(layout).sort(
      (a, b) => layout[a].y - layout[b].y || layout[a].x - layout[b].x
    );
    for (let i = 0; i < sorted.length; i++) {
      for (let j = i + 1; j < sorted.length; j++) {
        const a = layout[sorted[i]];
        const b = layout[sorted[j]];
        if (rectsOverlap(a, b)) {
          b.y = a.y + a.h;
          changed = true;
        }
      }
    }
    if (!changed) break;
  }
}

function getGridMetrics() {
  const grid = document.getElementById("dashboard");
  if (!grid) return { colWidth: 80, rowHeight: 96 };
  const rect = grid.getBoundingClientRect();
  const computedStyle = getComputedStyle(grid);
  const gap = parseFloat(computedStyle.gap) || 16;
  const colWidth = (rect.width - gap * 11) / 12;
  const rowHeight = 80 + gap; // 80px row + gap
  return { colWidth: colWidth + gap, rowHeight, gap };
}

function startDrag(e, cardId) {
  const card = document.getElementById(cardId);
  if (!card) return;
  const pos = { ...state.gridLayout[cardId] };
  if (!pos) return;

  _gridDragState = {
    cardId,
    startX: e.clientX,
    startY: e.clientY,
    origX: pos.x,
    origY: pos.y,
  };

  card.classList.add("dragging");

  const onMouseMove = (e) => {
    if (!_gridDragState) return;
    const metrics = getGridMetrics();
    const dx = e.clientX - _gridDragState.startX;
    const dy = e.clientY - _gridDragState.startY;
    const deltaCols = Math.round(dx / metrics.colWidth);
    const deltaRows = Math.round(dy / metrics.rowHeight);

    const pos = state.gridLayout[cardId];
    let newX = _gridDragState.origX + deltaCols;
    let newY = _gridDragState.origY + deltaRows;

    // Clamp
    newX = Math.max(1, Math.min(newX, 13 - pos.w));
    newY = Math.max(1, newY);

    pos.x = newX;
    pos.y = newY;
    resolveCollisions(cardId);
    applyLayout(state.gridLayout);
  };

  const onMouseUp = () => {
    if (_gridDragState) {
      card.classList.remove("dragging");
      _gridDragState = null;
      debouncedSaveLayout();
    }
    document.removeEventListener("mousemove", onMouseMove);
    document.removeEventListener("mouseup", onMouseUp);
  };

  document.addEventListener("mousemove", onMouseMove);
  document.addEventListener("mouseup", onMouseUp);
}

function startResize(e, cardId) {
  const card = document.getElementById(cardId);
  if (!card) return;
  const pos = { ...state.gridLayout[cardId] };
  if (!pos) return;

  _gridResizeState = {
    cardId,
    startX: e.clientX,
    startY: e.clientY,
    origW: pos.w,
    origH: pos.h,
  };

  card.classList.add("dragging");

  const onMouseMove = (e) => {
    if (!_gridResizeState) return;
    const metrics = getGridMetrics();
    const dx = e.clientX - _gridResizeState.startX;
    const dy = e.clientY - _gridResizeState.startY;
    const deltaCols = Math.round(dx / metrics.colWidth);
    const deltaRows = Math.round(dy / metrics.rowHeight);

    const pos = state.gridLayout[cardId];
    let newW = _gridResizeState.origW + deltaCols;
    let newH = _gridResizeState.origH + deltaRows;

    // Clamp
    newW = Math.max(2, Math.min(newW, 13 - pos.x));
    newH = Math.max(2, newH);

    pos.w = newW;
    pos.h = newH;
    resolveCollisions(cardId);
    applyLayout(state.gridLayout);
  };

  const onMouseUp = () => {
    if (_gridResizeState) {
      card.classList.remove("dragging");
      _gridResizeState = null;
      debouncedSaveLayout();
      // Trigger Chart.js resize
      if (state.charts.activity) state.charts.activity.resize();
      if (state.charts.pr) state.charts.pr.resize();
    }
    document.removeEventListener("mousemove", onMouseMove);
    document.removeEventListener("mouseup", onMouseUp);
  };

  document.addEventListener("mousemove", onMouseMove);
  document.addEventListener("mouseup", onMouseUp);
}

function loadGridLayout() {
  if (state.config && state.config.dashboard_layout && Object.keys(state.config.dashboard_layout).length > 0) {
    state.gridLayout = JSON.parse(JSON.stringify(state.config.dashboard_layout));
  } else {
    state.gridLayout = JSON.parse(JSON.stringify(DEFAULT_LAYOUT));
  }
}

function saveGridLayout() {
  if (!state.config || !state.gridLayout) return;
  state.config.dashboard_layout = JSON.parse(JSON.stringify(state.gridLayout));
  invoke("update_config", { config: state.config }).catch((err) => {
    console.error("Failed to save grid layout:", err);
  });
}

function debouncedSaveLayout() {
  if (_saveLayoutTimer) clearTimeout(_saveLayoutTimer);
  _saveLayoutTimer = setTimeout(saveGridLayout, 500);
}

function resetGridLayout() {
  state.gridLayout = JSON.parse(JSON.stringify(DEFAULT_LAYOUT));
  applyLayout(state.gridLayout);
  debouncedSaveLayout();
  showToast("Layout reset to defaults", "success");
}

// ===== Collapsible Card Sections =====

const COLLAPSED_CARDS_KEY = 'recap_collapsed_cards';

function getCollapsedCards() {
  try {
    const stored = localStorage.getItem(COLLAPSED_CARDS_KEY);
    return stored ? JSON.parse(stored) : {};
  } catch {
    return {};
  }
}

function setCardCollapsed(cardId, collapsed) {
  const collapsedCards = getCollapsedCards();
  if (collapsed) {
    collapsedCards[cardId] = true;
  } else {
    delete collapsedCards[cardId];
  }
  try {
    localStorage.setItem(COLLAPSED_CARDS_KEY, JSON.stringify(collapsedCards));
  } catch {}
}

function toggleCardCollapse(card) {
  const cardId = card.dataset.cardId;
  if (!cardId) return;
  const isCollapsed = card.classList.toggle('collapsed');
  setCardCollapsed(cardId, isCollapsed);
}

/**
 * Initializes collapsible behavior on all cards with data-card-id.
 * Wraps non-header children in a .card-collapsible-content div and
 * restores persisted collapsed state from localStorage.
 */
function initCollapsibleCards() {
  const collapsedState = getCollapsedCards();
  const cards = document.querySelectorAll('.card[data-card-id]');

  cards.forEach(card => {
    const cardId = card.dataset.cardId;

    // Avoid re-initializing if already wrapped
    if (card.querySelector('.card-collapsible-content')) return;

    // Collect all children after the card-header into a wrapper
    const header = card.querySelector('.card-header');
    if (!header) return;

    const wrapper = document.createElement('div');
    wrapper.className = 'card-collapsible-content';

    // Move all siblings after header into the wrapper
    const children = [...card.children];
    let afterHeader = false;
    for (const child of children) {
      if (child === header) {
        afterHeader = true;
        continue;
      }
      if (afterHeader) {
        wrapper.appendChild(child);
      }
    }
    card.appendChild(wrapper);

    // Restore collapsed state
    if (collapsedState[cardId]) {
      card.classList.add('collapsed');
    }

    // Click on the header toggles collapse
    header.addEventListener('click', (e) => {
      // Don't toggle if clicking a link, button (other than the toggle), or input inside header
      if (e.target.closest('a, input, select')) return;
      toggleCardCollapse(card);
    });
  });
}

// ===== Constants =====

const SOURCE_META = {
  github: {
    label: "GitHub",
    colorVar: '--github',
    helper: 'Or run: <code>gh auth login</code> and Recap will use it automatically.',
    placeholder: "ghp_...",
  },
  linear: {
    label: "Linear",
    colorVar: '--linear',
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
  issue_opened: "Issue Opened",
  issue_closed: "Issue Closed",
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
  issue_closed: "kind-closed",
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

/**
 * Clone the robot SVG from the <template id="robot-placeholder"> element.
 * Pass variant="confused" to swap the default straight mouth for the
 * squiggly confused-mouth + question marks used in disconnected states.
 */
function cloneRobotSvg(variant) {
  const tpl = document.getElementById('robot-placeholder');
  const svg = document.importNode(tpl.content, true).querySelector('svg');
  if (variant === 'confused') {
    // Replace the default straight mouth with a squiggly confused mouth
    const mouth = svg.querySelector('.robot-mouth');
    if (mouth) {
      mouth.innerHTML = '';
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      path.setAttribute('d', 'M45 76 Q50 72 55 76 Q60 80 65 76 Q70 72 75 76');
      path.setAttribute('stroke', 'var(--border)');
      path.setAttribute('stroke-width', '2.5');
      path.setAttribute('fill', 'none');
      path.setAttribute('stroke-linecap', 'round');
      mouth.appendChild(path);
    }
    // Add question marks on either side
    const q1 = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    q1.setAttribute('x', '22'); q1.setAttribute('y', '46');
    q1.setAttribute('fill', 'var(--text-dim)');
    q1.setAttribute('font-size', '16'); q1.setAttribute('font-weight', 'bold');
    q1.textContent = '?';
    const q2 = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    q2.setAttribute('x', '92'); q2.setAttribute('y', '46');
    q2.setAttribute('fill', 'var(--text-dim)');
    q2.setAttribute('font-size', '16'); q2.setAttribute('font-weight', 'bold');
    q2.textContent = '?';
    svg.appendChild(q1);
    svg.appendChild(q2);
  }
  return svg;
}

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
    actionEditLayout: $("#action-edit-layout"),
    actionResetLayout: $("#action-reset-layout"),
    actionBar: $("#action-bar"),

    // View sections
    viewOverview: $("#view-overview"),
    viewGithub: $("#view-github"),
    viewLinear: $("#view-linear"),
    viewSlack: $("#view-slack"),
    viewTrends: $("#view-trends"),
    trendsAiSummary: $("#trends-ai-summary"),
    trendsPredictions: $("#trends-predictions"),
    anomalyAlerts: $("#anomaly-alerts"),
    chartForecast: $("#chart-forecast"),
    chartProductivity: $("#chart-productivity"),
    chartVelocity: $("#chart-velocity"),
    chartCycleTime: $("#chart-cycle-time"),
    clusterContainer: $("#cluster-container"),
    projectPredContainer: $("#project-prediction-container"),
    heatmapContainer: $("#heatmap-container"),
    chartFocus: $("#chart-focus"),
    chartBurnout: $("#chart-burnout"),
    trendsAdvancedToggle: $("#trends-advanced-toggle"),

    // Standup modal
    standupOverlay: $("#standup-overlay"),
    standupClose: $("#standup-close"),
    standupTitle: $("#standup-title"),
    standupModalContent: $("#standup-modal-content"),
    standupCopy: $("#standup-copy"),

    // Settings modal
    settingsOverlay: $("#settings-overlay"),
    settingsClose: $("#settings-close"),
    settingsAuthList: $("#settings-auth-list"),
    settingsPrefs: $("#settings-prefs"),

    // Modals (for ESC-to-close)
    confirmOverlay: $("#confirm-overlay"),
    heatmapDetailOverlay: $("#heatmap-detail-overlay"),

    // GitHub view
    githubStats: $("#github-stats"),
    githubPrTbody: $("#github-pr-tbody"),
    githubPrCount: $("#github-pr-count"),
    githubCommitTbody: $("#github-commit-tbody"),
    githubCommitCount: $("#github-commit-count"),
    githubReviewTbody: $("#github-review-tbody"),
    githubReviewCount: $("#github-review-count"),
    githubOpenPrTbody: $("#github-open-pr-tbody"),
    githubOpenPrCount: $("#github-open-pr-count"),
    githubIssueTbody: $("#github-issue-tbody"),
    githubIssueCount: $("#github-issue-count"),

    // Linear view
    linearStats: $("#linear-stats"),
    linearIssueTbody: $("#linear-issue-tbody"),
    linearOpenTbody: $("#linear-open-tbody"),
    linearOpenCount: $("#linear-open-count"),
    linearIssueCount: $("#linear-issue-count"),

    // Slack view
    slackStats: $("#slack-stats"),
    slackChannels: $("#slack-channels"),
    slackMessageTbody: $("#slack-message-tbody"),
    slackMessageCount: $("#slack-message-count"),
  };

  // Inject robot SVGs into "coming soon" placeholders from template
  for (const id of ['slack-coming-soon', 'notion-coming-soon']) {
    const el = document.getElementById(id);
    if (el) el.prepend(cloneRobotSvg());
  }

  initKindColors();
  renderDate();
  bindEvents();
  initCollapsibleCards();
  bindUpdateBannerEvents();
  await refreshAuthStatus();
  // Load config first so grid layout can read it
  try {
    state.config = await invoke('get_config');
  } catch {}
  updateProfileModeUI();
  await loadDashboard();

  // Check for updates after UI is loaded
  setTimeout(checkForUpdates, 5000);
});

function bindEvents() {
  // External link handler — Tauri blocks navigation, so open in default browser
  document.addEventListener('click', (e) => {
    const link = e.target.closest('a[href]');
    if (!link) return;
    const href = link.getAttribute('href');
    if (href && (href.startsWith('http://') || href.startsWith('https://'))) {
      e.preventDefault();
      if (window.__TAURI__?.opener?.openUrl) {
        window.__TAURI__.opener.openUrl(href);
      } else {
        // Fallback for older Tauri versions
        invoke('plugin:opener|open_url', { url: href }).catch(() => {
          window.open(href, '_blank');
        });
      }
    }
  });

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
  dom.syncBtn.addEventListener("mouseenter", updateSyncTooltip);
  updateSyncTooltip();

  // Info tooltips: position fixed on hover to escape overflow:hidden
  document.addEventListener("mouseenter", (e) => {
    const icon = e.target.closest(".info-icon");
    if (!icon) return;
    const tip = icon.querySelector(".info-tooltip");
    if (!tip) return;
    const rect = icon.getBoundingClientRect();
    tip.style.display = "block";
    tip.style.top = (rect.bottom + 6) + "px";
    tip.style.left = Math.max(8, Math.min(rect.left - 130 + 9, window.innerWidth - 290)) + "px";
  }, true);
  document.addEventListener("mouseleave", (e) => {
    const icon = e.target.closest(".info-icon");
    if (!icon) return;
    const tip = icon.querySelector(".info-tooltip");
    if (tip) tip.style.display = "none";
  }, true);

  // Briefing refresh
  dom.briefingRefresh.addEventListener("click", () => {
    const key = briefingCacheKey();
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

  // Action bar: edit layout toggle
  dom.actionEditLayout.addEventListener("click", () => {
    toggleEditMode();
  });

  // Action bar: reset layout
  dom.actionResetLayout.addEventListener("click", () => {
    resetGridLayout();
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

  // ESC key closes open modals (topmost first)
  const modals = [dom.confirmOverlay, dom.heatmapDetailOverlay, dom.standupOverlay, dom.settingsOverlay];
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    for (const m of modals) {
      if (m && m.style.display !== "none") {
        m.style.display = "none";
        e.stopPropagation();
        return;
      }
    }
  });

  dom.trendsAdvancedToggle?.addEventListener("click", () => {
    state.trendsAdvanced = !state.trendsAdvanced;
    applyTrendsProfileMode();
  });
}

function isPersonalProfile() {
  return (state.config?.llm?.profile || "work") === "personal";
}

function setHidden(el, hidden) {
  if (!el) return;
  el.classList.toggle("hidden", !!hidden);
}

function applyOverviewProfileMode() {
  const personal = isPersonalProfile();
  setHidden(document.getElementById("card-pr-stats"), personal);
  setHidden(document.getElementById("card-linear"), personal);
}

function applyTrendsProfileMode() {
  const personal = isPersonalProfile();
  const toggle = dom.trendsAdvancedToggle;
  if (!toggle) return;

  const advancedTargets = [
    dom.anomalyAlerts,
    dom.chartForecast?.closest(".card"),
    dom.chartVelocity?.closest(".card"),
    dom.chartCycleTime?.closest(".card"),
    document.getElementById("cluster-container")?.closest(".card"),
    document.getElementById("project-prediction-container")?.closest(".card"),
    dom.chartFocus?.closest(".card"),
    dom.chartBurnout?.closest(".card"),
  ];

  if (!personal) {
    toggle.classList.add("hidden");
    advancedTargets.forEach((el) => setHidden(el, false));
    return;
  }

  toggle.classList.remove("hidden");
  toggle.innerHTML = state.trendsAdvanced ? "Advanced &#9652;" : "Advanced &#9662;";
  advancedTargets.forEach((el) => setHidden(el, !state.trendsAdvanced));
}

function updateProfileModeUI() {
  const personal = isPersonalProfile();
  if (dom.actionStandup) {
    dom.actionStandup.innerHTML = personal
      ? "&#x1f4dd; Generate Recap"
      : "&#x1f4cb; Generate Standup";
  }
  if (dom.standupTitle) {
    dom.standupTitle.textContent = personal ? "Personal Recap" : "Daily Standup";
  }
  applyOverviewProfileMode();
  applyTrendsProfileMode();
}

// ===== View Switching =====

function switchView(view) {
  state.activeView = view;
  // Dismiss update banner on navigation
  dismissUpdateBanner();
  // Exit edit mode if leaving overview
  if (view !== 'overview' && state.gridEditMode) {
    toggleEditMode();
  }
  // Hide all view sections
  document.querySelectorAll('.view-section').forEach(s => s.style.display = 'none');
  // Show target
  const el = document.getElementById(`view-${view}`);
  if (el) el.style.display = '';
  // Hide date controls + period tabs on Trends/coming-soon tabs
  const dateNav = document.querySelector('.date-nav');
  const tabBar = document.getElementById('tab-bar');
  const hideDateControls = view === 'trends' || view === 'slack' || view === 'notion';
  if (hideDateControls) {
    if (dateNav) { dateNav.style.visibility = 'hidden'; dateNav.style.pointerEvents = 'none'; }
    if (tabBar) { tabBar.style.visibility = 'hidden'; tabBar.style.pointerEvents = 'none'; }
  } else {
    if (dateNav) { dateNav.style.visibility = ''; dateNav.style.pointerEvents = ''; }
    if (tabBar) { tabBar.style.visibility = ''; tabBar.style.pointerEvents = ''; }
  }
  // Edit layout only applies to overview
  dom.actionEditLayout.style.display = view === 'overview' ? '' : 'none';
  // Load data (skip for coming-soon views)
  if (view !== 'slack' && view !== 'notion') loadViewData(view);
}

function loadViewData(view) {
  if (view === 'overview') {
    loadDashboard();
    return;
  }
  if (view === 'trends') {
    loadTrendsView();
    return;
  }
  // Source views need digest data
  if (!state.digest) {
    loadDashboard().then(() => renderSourceView(view));
  } else {
    renderSourceView(view);
  }
}

function renderNotConnectedPlaceholder(container, serviceName) {
  container.innerHTML = '';
  const wrapper = document.createElement('div');
  wrapper.className = 'coming-soon-view';
  const content = document.createElement('div');
  content.className = 'coming-soon-content';
  content.appendChild(cloneRobotSvg('confused'));
  const textDiv = document.createElement('div');
  textDiv.className = 'coming-soon-text';
  textDiv.innerHTML = `
    <h2>${escapeHtml(serviceName)} Not Connected</h2>
    <p class="coming-soon-badge" style="background:var(--highlight)">Not Connected</p>
    <p class="coming-soon-desc">Connect ${escapeHtml(serviceName)} in Settings to see your activity here.</p>
    <button class="btn" style="margin-top:12px" onclick="document.getElementById('action-settings').click()">Open Settings</button>`;
  content.appendChild(textDiv);
  wrapper.appendChild(content);
  container.appendChild(wrapper);
}

function ensureDisconnectedOverlay(viewId, serviceName) {
  const section = document.getElementById(`view-${viewId}`);
  if (!section) return null;
  let overlay = section.querySelector('.disconnected-overlay');
  if (!overlay) {
    overlay = document.createElement('div');
    overlay.className = 'disconnected-overlay';
    renderNotConnectedPlaceholder(overlay, serviceName);
    section.appendChild(overlay);
  }
  return overlay;
}

function renderSourceView(view) {
  const activities = state.digest?.activities || [];

  // Show "not connected" placeholder if the integration is disconnected
  const checks = { github: 'GitHub', linear: 'Linear' };
  if (checks[view] && !state.authStatus[view]) {
    const section = document.getElementById(`view-${view}`);
    if (section) {
      const sourceView = section.querySelector('.source-view');
      const overlay = ensureDisconnectedOverlay(view, checks[view]);
      if (sourceView) sourceView.style.display = 'none';
      if (overlay) overlay.style.display = '';
    }
    return;
  }
  // Restore source view if previously hidden
  if (checks[view]) {
    const section = document.getElementById(`view-${view}`);
    if (section) {
      const sourceView = section.querySelector('.source-view');
      const overlay = section.querySelector('.disconnected-overlay');
      if (sourceView) sourceView.style.display = '';
      if (overlay) overlay.style.display = 'none';
    }
  }

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
  // Auth warnings are now shown only in the Settings modal
  dom.authBanner.style.display = "none";
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

function llmProfileKey() {
  return state.config?.llm?.profile || "work";
}

function briefingCacheKey() {
  return `${llmProfileKey()}:${cacheKey()}`;
}

function standupCacheKey() {
  return `${llmProfileKey()}:${cacheKey()}`;
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
      applyOverviewProfileMode();
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
    applyOverviewProfileMode();

    // Initialize 12-column grid layout
    initGridSystem();

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

  const personal = isPersonalProfile();
  const total = stats.total_activities || 0;
  const merged = stats.by_kind?.["pr_merged"] || 0;
  const reviewed = stats.by_kind?.["pr_reviewed"] || 0;
  const issuesCompleted = stats.by_kind?.["issue_completed"] || 0;
  const commits = stats.by_kind?.["commit_pushed"] || 0;
  const issuesClosed = stats.by_kind?.["issue_closed"] || 0;

  const set = (id, value) => {
    const el = document.querySelector(`#${id} .headline-value`);
    if (el) el.textContent = value;
  };

  set("hl-total", total);
  set("hl-prs", personal ? commits : merged);
  set("hl-reviews", personal ? (merged + reviewed) : reviewed);
  set("hl-issues", personal ? (issuesCompleted + issuesClosed) : issuesCompleted);

  const setLabel = (id, value) => {
    const el = document.querySelector(`#${id} .headline-label`);
    if (el) el.textContent = value;
  };
  setLabel("hl-total", personal ? "Activity Signals" : "Total Activities");
  setLabel("hl-prs", personal ? "Commits" : "PRs Merged");
  setLabel("hl-reviews", personal ? "PR Activity" : "Reviews");
  setLabel("hl-issues", personal ? "Issues Closed" : "Issues Completed");
}

// ===== Activity Over Time Chart =====

function showNoData(canvas, message) {
  if (!canvas) return;
  canvas.style.display = 'none';
  let noData = canvas.parentElement.querySelector('.no-data');
  if (!noData) {
    noData = document.createElement('div');
    noData.className = 'no-data';
    canvas.parentElement.appendChild(noData);
  }
  noData.textContent = message;
  noData.style.display = '';
}

function restoreCanvas(canvas) {
  if (!canvas) return;
  canvas.style.display = '';
  const noData = canvas.parentElement?.querySelector('.no-data');
  if (noData) noData.style.display = 'none';
}

function renderActivityChart(chartData) {
  if (!chartData || !chartData.labels || !chartData.datasets) {
    showNoData(dom.chartActivity, 'No chart data available');
    return;
  }

  if (!hasChartJs()) {
    dom.chartActivity.parentElement.innerHTML = renderFallbackStats(chartData);
    return;
  }
  restoreCanvas(dom.chartActivity);

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
            color: cssVar('--text-dim'),
            font: { size: 11 },
            boxWidth: 12,
            padding: 12,
          },
        },
      },
      scales: {
        x: {
          stacked: true,
          ticks: { color: cssVar('--text-dim'), font: { size: 11 } },
          grid: { display: false },
        },
        y: {
          stacked: true,
          ticks: { color: cssVar('--text-dim'), font: { size: 11 }, stepSize: 1 },
          grid: { color: cssVar('--chart-grid') },
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
  const isTrunk = state.config?.github?.workflow === 'trunk';

  // Store on state for filter callbacks
  state._ghPrs = activities.filter(a => ['pr_opened', 'pr_merged'].includes(a.kind));
  state._ghCommits = activities.filter(a => a.kind === 'commit_pushed');
  state._ghReviews = activities.filter(a => a.kind === 'pr_reviewed');

  const prs = state._ghPrs;
  const commits = state._ghCommits;
  const reviews = state._ghReviews;
  const merged = prs.filter(p => p.kind === 'pr_merged').length;
  const opened = prs.filter(p => p.kind === 'pr_opened').length;

  // Stats row — reorder based on workflow
  const statCards = isTrunk
    ? [
        { value: commits.length, label: 'Commits' },
        { value: prs.length, label: 'Pull Requests' },
        { value: reviews.length, label: 'Reviews' },
        { value: merged, label: 'Merged' },
      ]
    : [
        { value: prs.length, label: 'Pull Requests' },
        { value: merged, label: 'Merged' },
        { value: reviews.length, label: 'Reviews' },
        { value: commits.length, label: 'Commits' },
      ];
  dom.githubStats.innerHTML = renderStatCards(statCards);

  // Set card order via CSS: trunk puts commits first, PR-based puts PRs first
  const prCard = document.getElementById('github-card-prs');
  const commitCard = document.getElementById('github-card-commits');
  if (prCard && commitCard) {
    prCard.style.order = isTrunk ? 2 : 1;
    commitCard.style.order = isTrunk ? 1 : 2;
  }

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

  // Fetch open/draft PRs and issues (independent of date range)
  fetchOpenPrs();
  fetchGitHubIssues();
}

async function fetchOpenPrs() {
  dom.githubOpenPrTbody.innerHTML = '<tr><td colspan="5" class="no-data">Loading...</td></tr>';
  dom.githubOpenPrCount.textContent = '';
  try {
    const prs = await invoke('get_open_prs');
    dom.githubOpenPrCount.textContent = `${prs.length} PRs`;
    if (!prs.length) {
      dom.githubOpenPrTbody.innerHTML = '<tr><td colspan="5" class="no-data">No open PRs</td></tr>';
      return;
    }
    dom.githubOpenPrTbody.innerHTML = prs.map(pr => {
      const isDraft = pr.state === 'draft';
      const statusClass = isDraft ? 'kind-backlog' : 'kind-open';
      const statusLabel = isDraft ? 'Draft' : 'Open';
      const ccHtml = pr.cc_type
        ? `<span class="cc-tag cc-${pr.cc_type}">${pr.cc_type}${pr.cc_scope ? `(${escapeHtml(pr.cc_scope)})` : ''}</span>`
        : '<span style="color:var(--text-muted)">—</span>';
      return `<tr>
        <td><span class="kind-badge ${statusClass}">${statusLabel}</span></td>
        <td>${ccHtml}</td>
        <td style="max-width:400px">${pr.url ? `<a class="activity-title-link" href="${escapeAttr(pr.url)}" target="_blank">${escapeHtml(pr.title)}</a>` : escapeHtml(pr.title)}</td>
        <td><span class="project-tag">${escapeHtml(pr.repo)}</span></td>
        <td><span class="time-dim">${relativeTime(pr.updated_at)}</span></td>
      </tr>`;
    }).join('');
  } catch (err) {
    dom.githubOpenPrTbody.innerHTML = `<tr><td colspan="5" class="no-data">${escapeHtml(String(err))}</td></tr>`;
  }
}

async function fetchGitHubIssues() {
  dom.githubIssueTbody.innerHTML = '<tr><td colspan="4" class="no-data">Loading...</td></tr>';
  dom.githubIssueCount.textContent = '';
  try {
    const issues = await invoke('get_github_issues');
    dom.githubIssueCount.textContent = `${issues.length} issues`;
    if (!issues.length) {
      dom.githubIssueTbody.innerHTML = '<tr><td colspan="4" class="no-data">No open issues</td></tr>';
      return;
    }
    dom.githubIssueTbody.innerHTML = issues.map(issue => {
      const labelsHtml = issue.labels.length
        ? issue.labels.map(l => {
            const bg = `#${l.color}`;
            const textColor = labelContrastColor(l.color);
            return `<span class="cc-tag gh-label" style="background:${bg};color:${textColor}">${escapeHtml(l.name)}</span>`;
          }).join(' ')
        : '';
      return `<tr>
        <td><span class="kind-badge kind-open">Open</span> ${labelsHtml}</td>
        <td style="max-width:400px">${issue.url ? `<a class="activity-title-link" href="${escapeAttr(issue.url)}" target="_blank">${escapeHtml(issue.title)}</a>` : escapeHtml(issue.title)}</td>
        <td><span class="project-tag">${escapeHtml(issue.repo)}</span></td>
        <td><span class="time-dim">${relativeTime(issue.updated_at)}</span></td>
      </tr>`;
    }).join('');
  } catch (err) {
    dom.githubIssueTbody.innerHTML = `<tr><td colspan="4" class="no-data">${escapeHtml(String(err))}</td></tr>`;
  }
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

  // Fetch open tickets (independent of date range)
  fetchOpenTickets();
}

async function fetchOpenTickets() {
  dom.linearOpenTbody.innerHTML = '<tr><td colspan="6" class="no-data">Loading...</td></tr>';
  dom.linearOpenCount.textContent = '';
  try {
    const tickets = await invoke('get_open_tickets');
    dom.linearOpenCount.textContent = `${tickets.length} tickets`;
    if (!tickets.length) {
      dom.linearOpenTbody.innerHTML = '<tr><td colspan="6" class="no-data">No open tickets</td></tr>';
      return;
    }
    dom.linearOpenTbody.innerHTML = tickets.map(t => {
      const stateClass = t.state_type === 'started' ? 'state-started' : 'state-backlog';
      const priorityClass = t.priority_label.toLowerCase().includes('urgent') ? 'urgent'
        : t.priority_label.toLowerCase().includes('high') ? 'high'
        : t.priority_label.toLowerCase().includes('medium') ? 'medium' : 'low';
      return `<tr>
        <td>${t.url ? `<a href="${escapeAttr(t.url)}" target="_blank" class="kind-badge kind-issue" style="text-decoration:none">${escapeHtml(t.identifier)}</a>` : `<span class="kind-badge kind-issue">${escapeHtml(t.identifier)}</span>`}</td>
        <td style="max-width:300px">${t.url ? `<a class="activity-title-link" href="${escapeAttr(t.url)}" target="_blank">${escapeHtml(t.title)}</a>` : escapeHtml(t.title)}</td>
        <td><span class="state-badge ${stateClass}">${escapeHtml(t.state)}</span></td>
        <td><span class="priority-badge ${priorityClass}">${escapeHtml(t.priority_label)}</span></td>
        <td>${t.team ? escapeHtml(t.team) : '—'}</td>
        <td><span class="time-dim">${relativeTime(t.updated_at)}</span></td>
      </tr>`;
    }).join('');
  } catch (err) {
    dom.linearOpenTbody.innerHTML = `<tr><td colspan="6" class="no-data">${escapeHtml(String(err))}</td></tr>`;
  }
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
  const key = briefingCacheKey();

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
  const key = standupCacheKey();
  const personal = isPersonalProfile();
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
    dom.standupModalContent.innerHTML = standup
      ? renderMarkdown(standup)
      : `<span class="briefing-disabled">Could not generate ${personal ? "recap" : "standup"}. Is Claude CLI installed?</span>`;
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
  updateProfileModeUI();
  await refreshAuthStatus();
  renderUpdateStatus();
  renderSettingsConnections();
  renderSettingsPrefs();
}

async function renderUpdateStatus() {
  const el = document.getElementById('settings-update-status');
  if (!el) return;

  let currentVersion = '0.0.0';
  try {
    // Tauri v2: getVersion() is async
    if (window.__TAURI__?.app?.getVersion) {
      currentVersion = await window.__TAURI__.app.getVersion();
    }
  } catch {}

  const status = state.updateStatus || 'checking';
  let dotClass, label, extra = '';

  switch (status) {
    case 'up_to_date':
      dotClass = 'rag-green';
      label = 'Up to date';
      break;
    case 'available':
      dotClass = 'rag-amber';
      label = `Update available: v${state.updateVersion}`;
      extra = `<button class="btn btn-small btn-highlight" id="settings-install-update" style="margin-left:8px">Install &amp; Restart</button>`;
      break;
    case 'error':
      dotClass = 'rag-red';
      label = 'Update check failed';
      break;
    case 'checking':
    default:
      dotClass = 'rag-checking';
      label = 'Checking for updates...';
      break;
  }

  const errorDetail = status === 'error' ? `
    <div class="update-error-detail">
      <span class="update-error-text">${escapeHtml(state.updateError || 'Unknown error')}</span>
      <button class="btn btn-tiny" id="settings-copy-update-error" title="Copy error to clipboard">Copy Error</button>
    </div>` : '';

  el.innerHTML = `
    <div class="update-status-row">
      <div class="update-status-left">
        <span class="rag-dot ${dotClass}"></span>
        <span class="update-version">v${escapeHtml(currentVersion)}</span>
        <span class="update-label">${label}</span>
        ${extra}
      </div>
      <button class="btn btn-tiny" id="settings-check-update" title="Check now">Check</button>
    </div>${errorDetail}`;

  document.getElementById('settings-check-update')?.addEventListener('click', async (e) => {
    e.target.disabled = true;
    e.target.textContent = '...';
    await checkForUpdates(true);
    renderUpdateStatus();
  });

  document.getElementById('settings-install-update')?.addEventListener('click', installUpdate);

  document.getElementById('settings-copy-update-error')?.addEventListener('click', async (e) => {
    try {
      await navigator.clipboard.writeText(state.updateError || 'Unknown error');
      e.target.textContent = 'Copied';
      setTimeout(() => { e.target.textContent = 'Copy Error'; }, 1500);
    } catch {
      showToast('Failed to copy to clipboard', 'error');
    }
  });
}

function renderSettingsConnections() {
  let html = '';
  // Show disconnected-services warning inside settings
  const sources = ["github", "linear", "slack", "notion"];
  const disconnected = sources.filter((s) => !state.authStatus[s]);
  if (disconnected.length > 0) {
    const names = disconnected.map((s) => SOURCE_META[s]?.label || s).join(", ");
    html += `<div class="settings-auth-warning">&#x26a0; ${disconnected.length} service${disconnected.length > 1 ? "s" : ""} not connected (${names})</div>`;
  }
  for (const [source, meta] of Object.entries(SOURCE_META)) {
    const connected = state.authStatus[source];
    html += `<div class="auth-source">
      <div class="auth-dot ${connected ? 'connected' : 'disconnected'}"></div>
      <div class="auth-info">
        <div class="auth-name">${meta.label}</div>
        ${connected
          ? '<div class="auth-helper" style="color:var(--status-completed);">Connected</div>'
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
  // Anthropic API key (for LLM summaries when claude CLI is unavailable)
  const anthropicConnected = state.authStatus.anthropic;
  html += `<div class="auth-source">
    <div class="auth-dot ${anthropicConnected ? 'connected' : 'disconnected'}"></div>
    <div class="auth-info">
      <div class="auth-name">Anthropic</div>
      ${anthropicConnected
        ? '<div class="auth-helper" style="color:var(--status-completed);">API key configured</div>'
        : `<div class="auth-helper">Required for LLM summaries &amp; standup generation. Get a key from the Anthropic Console.</div>
           <div class="auth-input-row">
             <input type="password" class="auth-input" id="token-anthropic" placeholder="sk-ant-..." autocomplete="off" spellcheck="false">
             <button class="btn btn-small btn-highlight" id="save-anthropic-key">Save</button>
           </div>`
      }
    </div>
  </div>`;

  dom.settingsAuthList.innerHTML = html;
  // Bind save buttons
  dom.settingsAuthList.querySelectorAll('[data-save-token]').forEach(btn => {
    btn.addEventListener('click', () => saveToken(btn.dataset.saveToken));
  });
  const anthropicBtn = dom.settingsAuthList.querySelector('#save-anthropic-key');
  if (anthropicBtn) {
    anthropicBtn.addEventListener('click', async () => {
      const key = document.getElementById('token-anthropic')?.value?.trim();
      if (!key) { showToast('Please enter an API key.', 'error'); return; }
      try {
        await invoke('save_anthropic_key', { key });
        showToast('Anthropic API key saved!', 'success');
        await refreshAuthStatus();
        renderSettingsConnections();
      } catch (err) {
        showToast(`Failed to save key: ${err}`, 'error');
      }
    });
  }
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
  const llm = c.llm || {};
  const personalProfile = (llm.profile || "work") === "personal";
  const wh = c.working_hours || {};
  const workingDays = wh.working_days || ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'];
  const allDays = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  const dayCheckboxes = allDays.map(d =>
    `<label class="pref-day-label"><input type="checkbox" class="pref-day-cb" value="${d}"${workingDays.includes(d) ? ' checked' : ''}> ${d}</label>`
  ).join('');
  const workingHoursSection = personalProfile
    ? `<div class="pref-row">
         <div class="pref-hint">Working-hours settings are hidden in Personal mode. Switch to Work profile to configure burnout/working-hours analysis.</div>
       </div>`
    : `<div class="pref-section-divider">Working Hours</div>
       <div class="pref-row">
         <label class="pref-label">Work start</label>
         <input class="pref-input pref-input-time" type="time" id="pref-work-start" value="${escapeAttr(wh.work_start || '09:00')}">
       </div>
       <div class="pref-row">
         <label class="pref-label">Work end</label>
         <input class="pref-input pref-input-time" type="time" id="pref-work-end" value="${escapeAttr(wh.work_end || '17:00')}">
       </div>
       <div class="pref-row pref-row-days">
         <label class="pref-label">Working days</label>
         <div class="pref-days">${dayCheckboxes}</div>
       </div>
       <div class="pref-row">
         <label class="pref-label">Timezone</label>
         <input class="pref-input" type="text" id="pref-timezone" value="${escapeAttr(wh.timezone || 'UTC')}" placeholder="UTC, Europe/London, America/New_York…">
         <div class="pref-hint">IANA timezone name</div>
       </div>`;
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
      <label class="pref-label">GitHub workflow</label>
      <select class="pref-input" id="pref-gh-workflow">
        <option value="pr"${(c.github?.workflow || 'pr') === 'pr' ? ' selected' : ''}>PR-based (feature branches)</option>
        <option value="trunk"${c.github?.workflow === 'trunk' ? ' selected' : ''}>Trunk-based (commits to main)</option>
      </select>
      <div class="pref-hint">PR-based shows PRs first; trunk-based emphasizes commits</div>
    </div>
    <div class="pref-row">
      <label class="pref-label">LLM profile</label>
      <select class="pref-input" id="pref-llm-profile">
        <option value="work"${(llm.profile || 'work') === 'work' ? ' selected' : ''}>Work</option>
        <option value="personal"${llm.profile === 'personal' ? ' selected' : ''}>Personal</option>
      </select>
      <div class="pref-hint">Personal mode favors momentum/effort framing over velocity/burnout language</div>
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
    ${workingHoursSection}
    <div class="pref-save-row">
      <button class="btn" id="pref-save-btn">Save Preferences</button>
    </div>
    <div style="margin-top:20px;padding-top:16px;border-top:1px solid var(--border)">
      <div style="font-size:12px;color:var(--text-dim);margin-bottom:8px">Clear all synced activities, LLM summaries, and sync cursors. Tokens are kept.</div>
      <button class="btn btn-danger" id="pref-clear-cache-btn">Clear Cached Data</button>
    </div>
    <div style="margin-top:20px;padding-top:16px;border-top:1px solid var(--border)">
      <div class="pref-section-divider">Debug</div>
      <button class="btn" id="debug-show-activities-btn">Show All Activities</button>
    </div>
  `;
  document.getElementById('pref-save-btn')?.addEventListener('click', savePreferences);
  const clearBtn = document.getElementById('pref-clear-cache-btn');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      showConfirmDialog(
        'Clear Cached Data',
        'This will remove all synced activities, LLM summaries, and sync cursors. Your API tokens will be kept. Are you sure?',
        async () => {
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
            showToast(`Failed to clear cache: ${err}`, 'error');
          } finally {
            clearBtn.disabled = false;
            setTimeout(() => { clearBtn.textContent = 'Clear Cached Data'; }, 2000);
          }
        }
      );
    });
  }

  // Debug: show all activities in a slide-out drawer
  const debugBtn = document.getElementById('debug-show-activities-btn');
  if (debugBtn) {
    debugBtn.addEventListener('click', async () => {
      // If drawer already open, close it
      const existing = document.getElementById('debug-drawer-overlay');
      if (existing) { existing.remove(); return; }

      debugBtn.textContent = 'Loading...';
      debugBtn.disabled = true;
      try {
        const activities = await invoke('get_all_activities');
        state._debugActivities = activities;

        // Build drawer overlay
        const overlay = document.createElement('div');
        overlay.id = 'debug-drawer-overlay';
        overlay.className = 'debug-drawer-overlay';
        overlay.innerHTML = `
          <div class="debug-drawer">
            <div class="debug-drawer-header">
              <h2>All Activities</h2>
              <button class="btn btn-icon" id="debug-drawer-close">&times;</button>
            </div>
            <div class="debug-drawer-filters">
              <input class="pref-input" type="text" id="debug-filter" placeholder="Filter by title, source, kind, project..." style="flex:1;min-width:200px">
              <select class="pref-input" id="debug-source-filter" style="width:auto">
                <option value="">All sources</option>
                <option value="github">GitHub</option>
                <option value="linear">Linear</option>
                <option value="slack">Slack</option>
                <option value="notion">Notion</option>
              </select>
              <select class="pref-input" id="debug-kind-filter" style="width:auto">
                <option value="">All kinds</option>
              </select>
              <span id="debug-count" style="font-size:11px;color:var(--text-dim);align-self:center"></span>
            </div>
            <div class="debug-drawer-body" id="debug-activities-table"></div>
          </div>`;
        document.body.appendChild(overlay);

        // Close drawer on overlay click or close button
        const closeDrawer = () => { overlay.remove(); };
        overlay.addEventListener('click', (e) => { if (e.target === overlay) closeDrawer(); });
        document.getElementById('debug-drawer-close')?.addEventListener('click', closeDrawer);

        // Close on Escape key
        const onKey = (e) => { if (e.key === 'Escape') { closeDrawer(); document.removeEventListener('keydown', onKey); } };
        document.addEventListener('keydown', onKey);

        // Populate kind filter with unique kinds
        const kindFilter = document.getElementById('debug-kind-filter');
        const kinds = [...new Set(activities.map(a => a.kind))].sort();
        kindFilter.innerHTML = '<option value="">All kinds</option>' +
          kinds.map(k => `<option value="${k}">${k}</option>`).join('');

        renderDebugActivities();

        // Bind filter events
        document.getElementById('debug-filter')?.addEventListener('input', renderDebugActivities);
        document.getElementById('debug-source-filter')?.addEventListener('change', renderDebugActivities);
        document.getElementById('debug-kind-filter')?.addEventListener('change', renderDebugActivities);
      } catch (err) {
        showToast(`Failed to load activities: ${err}`, 'error');
      } finally {
        debugBtn.textContent = 'Show All Activities';
        debugBtn.disabled = false;
      }
    });
  }
}

const DEBUG_PAGE_SIZE = 100;

function renderDebugActivities() {
  const activities = state._debugActivities || [];
  const textFilter = (document.getElementById('debug-filter')?.value || '').toLowerCase();
  const sourceFilter = document.getElementById('debug-source-filter')?.value || '';
  const kindFilter = document.getElementById('debug-kind-filter')?.value || '';

  const filtered = activities.filter(a => {
    if (sourceFilter && a.source !== sourceFilter) return false;
    if (kindFilter && a.kind !== kindFilter) return false;
    if (textFilter) {
      const haystack = `${a.title} ${a.source} ${a.kind} ${a.project || ''} ${a.source_id}`.toLowerCase();
      if (!haystack.includes(textFilter)) return false;
    }
    return true;
  });

  // Reset page when filters change
  state._debugPage = 1;
  state._debugFiltered = filtered;

  document.getElementById('debug-count').textContent = `${filtered.length} / ${activities.length} activities`;

  renderDebugPage();
}

function renderDebugPage() {
  const filtered = state._debugFiltered || [];
  const page = state._debugPage || 1;
  const visible = filtered.slice(0, page * DEBUG_PAGE_SIZE);
  const hasMore = visible.length < filtered.length;

  const table = document.getElementById('debug-activities-table');
  if (!filtered.length) {
    table.innerHTML = '<div class="no-data" style="padding:20px">No matching activities</div>';
    return;
  }

  table.innerHTML = `<table class="activity-table" style="font-size:11px">
    <thead><tr>
      <th style="width:70px">Source</th>
      <th style="width:110px">Kind</th>
      <th>Title</th>
      <th style="width:120px">Project</th>
      <th style="width:70px">Date</th>
      <th style="width:160px">Source ID</th>
    </tr></thead>
    <tbody>${visible.map(a => `<tr>
      <td><span style="color:var(--${a.source})">${escapeHtml(a.source)}</span></td>
      <td><span class="kind-badge ${KIND_CLASSES[a.kind] || ''}" style="font-size:9px">${KIND_LABELS[a.kind] || a.kind}</span></td>
      <td style="max-width:300px">${a.url ? `<a class="activity-title-link" href="${escapeAttr(a.url)}" target="_blank">${escapeHtml(a.title)}</a>` : escapeHtml(a.title)}</td>
      <td>${a.project ? `<span class="project-tag">${escapeHtml(a.project)}</span>` : '—'}</td>
      <td><span class="time-dim">${new Date(a.occurred_at).toLocaleDateString()}</span></td>
      <td style="font-family:monospace;font-size:9px;color:var(--text-dim);word-break:break-all">${escapeHtml(a.source_id)}</td>
    </tr>`).join('')}</tbody>
  </table>${hasMore ? `<div style="text-align:center;padding:8px">
    <button class="btn btn-small" id="debug-load-more">Show ${Math.min(DEBUG_PAGE_SIZE, filtered.length - visible.length)} more (${visible.length} / ${filtered.length})</button>
  </div>` : `<div style="text-align:center;padding:6px;font-size:11px;color:var(--text-dim)">Showing all ${filtered.length} activities</div>`}`;

  document.getElementById('debug-load-more')?.addEventListener('click', () => {
    state._debugPage = (state._debugPage || 1) + 1;
    renderDebugPage();
  });
}

async function savePreferences() {
  if (!state.config) return;
  const previousProfile = state.config.llm?.profile || "work";
  state.config.schedule.sync_interval_minutes = parseInt(document.getElementById('pref-sync-interval')?.value) || 5;
  state.config.schedule.daily_reminder_time = document.getElementById('pref-reminder-time')?.value || '17:00';
  const ghUsername = document.getElementById('pref-gh-username')?.value?.trim();
  state.config.github.username = ghUsername || null;
  state.config.github.workflow = document.getElementById('pref-gh-workflow')?.value || 'pr';
  if (!state.config.llm) state.config.llm = {};
  state.config.llm.profile = document.getElementById('pref-llm-profile')?.value || 'work';
  const profileChanged = previousProfile !== state.config.llm.profile;
  const slackUserId = document.getElementById('pref-slack-userid')?.value?.trim();
  state.config.slack.user_id = slackUserId || null;
  const ignoredStr = document.getElementById('pref-ignored-channels')?.value || '';
  state.config.slack.ignored_channels = ignoredStr.split(',').map(s => s.trim()).filter(Boolean);
  // Working hours
  if (!state.config.working_hours) state.config.working_hours = {};
  const workStartEl = document.getElementById('pref-work-start');
  const workEndEl = document.getElementById('pref-work-end');
  const timezoneEl = document.getElementById('pref-timezone');
  if (workStartEl && workEndEl && timezoneEl) {
    state.config.working_hours.work_start = workStartEl.value || '09:00';
    state.config.working_hours.work_end = workEndEl.value || '17:00';
    state.config.working_hours.timezone = timezoneEl.value?.trim() || 'UTC';
    const checkedDays = [...document.querySelectorAll('.pref-day-cb:checked')].map(cb => cb.value);
    state.config.working_hours.working_days = checkedDays;
  }
  try {
    await invoke('update_config', { config: state.config });
    if (profileChanged) {
      await invoke('clear_llm_cache');
      state.llmCache = {};
      state.standupCache = {};
      state.briefingText = null;
      state.standupText = null;
      state.trendsAdvanced = false;
      updateProfileModeUI();
      await loadDashboard();
      if (state.activeView === 'trends') {
        await loadTrendsView();
      }
    } else {
      updateProfileModeUI();
    }
    showToast('Preferences saved!', 'success');
  } catch (err) {
    showToast(`Failed to save: ${err}`, 'error');
  }
}

// ===== Sync =====

function updateSyncTooltip() {
  if (!state.lastSyncTime) {
    dom.syncBtn.title = "Never synced";
    return;
  }
  const ago = relativeTime(state.lastSyncTime.toISOString());
  dom.syncBtn.title = `Last synced: ${ago}`;
}

async function handleSync() {
  dom.syncBtn.disabled = true;
  dom.syncBtn.classList.add("syncing");
  dom.syncBtn.textContent = "Syncing...";

  try {
    const msg = await invoke("trigger_sync");
    state.lastSyncTime = new Date();
    updateSyncTooltip();
    showToast(msg || "Sync complete", "success");
    // Clear in-memory caches so summaries regenerate with fresh data
    state.llmCache = {};
    state.standupCache = {};
    state.briefingText = null;
    await refreshAuthStatus();
    await loadDashboard();
    // Retrigger briefing / summary generation with fresh data
    fetchBriefing();
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

function showConfirmDialog(title, message, onConfirm) {
  const overlay = document.getElementById('confirm-overlay');
  document.getElementById('confirm-title').textContent = title;
  document.getElementById('confirm-message').textContent = message;
  overlay.style.display = '';
  const okBtn = document.getElementById('confirm-ok');
  const cancelBtn = document.getElementById('confirm-cancel');
  const close = () => { overlay.style.display = 'none'; };
  const handleOk = () => { close(); onConfirm(); cleanup(); };
  const handleCancel = () => { close(); cleanup(); };
  const handleOverlay = (e) => { if (e.target === overlay) { close(); cleanup(); } };
  function cleanup() {
    okBtn.removeEventListener('click', handleOk);
    cancelBtn.removeEventListener('click', handleCancel);
    overlay.removeEventListener('click', handleOverlay);
  }
  okBtn.addEventListener('click', handleOk);
  cancelBtn.addEventListener('click', handleCancel);
  overlay.addEventListener('click', handleOverlay);
}

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

// ===== Trends View =====

const TREND_CHART_OPTS = {
  responsive: true,
  maintainAspectRatio: false,
  plugins: {
    legend: { position: 'bottom', labels: { color: cssVar('--text-dim'), font: { size: 11 }, boxWidth: 12, padding: 12 } },
  },
  scales: {
    x: { ticks: { color: cssVar('--text-dim'), font: { size: 10 } }, grid: { display: false } },
    y: { ticks: { color: cssVar('--text-dim'), font: { size: 11 } }, grid: { color: cssVar('--chart-grid') } },
  },
};

async function loadTrendsView() {
  try {
    const data = await invoke('get_trends_data');
    renderTrendsHeader(data.productivity, data.prediction);
    renderAnomalies(data.anomalies);
    renderForecastChart(data.velocity, data.prediction);
    renderProductivityChart(data.productivity);
    renderVelocityChart(data.velocity);
    renderCycleTimeChart(data.cycle_time);
    renderDayClusters(data.day_clusters);
    renderProjectPrediction(data.project_prediction);
    renderHeatmap(data.heatmap);
    renderFocusChart(data.focus);
    renderBurnoutChart(data.burnout);
    // Generate AI summary async (don't block)
    generateTrendsAiSummary(data);
    applyTrendsProfileMode();
  } catch (err) {
    showToast(`Failed to load trends: ${err}`, 'error');
  }
}

async function generateTrendsAiSummary(data) {
  dom.trendsAiSummary.innerHTML = '<div class="loading-dots"><span class="pulse-dot"></span><span class="pulse-dot"></span><span class="pulse-dot"></span></div>';

  // Build a compact data summary for the LLM
  const p = data.productivity;
  const f = data.prediction;
  const b = data.burnout;
  const anomalyDesc = data.anomalies.slice(0, 4).map(a =>
    `${a.kind} was ${a.direction} in ${a.week} (${a.value}, ${a.z_score}\u03c3)`
  ).join('; ') || 'none';
  const clusterDesc = data.day_clusters.clusters.map(c =>
    `${c.name}: ${c.count} days`
  ).join(', ');
  const projPred = data.project_prediction.slice(0, 3).map(pp =>
    `${pp.project} (${Math.round(pp.probability * 100)}%)`
  ).join(', ');
  const focusProjects = Object.keys(data.focus.projects).join(', ');
  const llmProfile = state.config?.llm?.profile || 'work';

  const forecastLines = Object.entries(f.forecasts).map(([kind, vals]) =>
    `${kind}: ${vals.map(v => v.toFixed(1)).join(', ')}`
  ).join('; ');

  const prompt = llmProfile === 'personal'
    ? `You are analyzing a personal developer's 12-week activity trends. Give a concise, insightful 3-4 bullet analysis in markdown.
Focus on momentum, consistency, and craft development. Mention effort signals carefully (sustained focus/time investment) only when evidence is clear.
Avoid corporate performance framing and do not over-index on burnout language.

Momentum: current score ${p.current_score}, baseline avg ${p.baseline_avg}, trend ${p.trend}
Effort note: off-hours trend ${b.trend_direction}, latest off-hours ${b.off_hours_pct.slice(-1)[0]?.toFixed(1) || 0}%
Forecast (next 3 weeks): ${forecastLines}
Anomalies: ${anomalyDesc}
Day types: ${clusterDesc}
Tomorrow prediction: ${projPred}
Active projects: ${focusProjects}`
    : `You are analyzing a software engineer's 12-week activity trends. Give a concise, insightful 3-4 bullet analysis in markdown. Be specific with numbers. Highlight what's notable — don't just restate data.

Productivity: current score ${p.current_score}, baseline avg ${p.baseline_avg}, trend ${p.trend}
Burnout: off-hours trend ${b.trend_direction}, latest off-hours ${b.off_hours_pct.slice(-1)[0]?.toFixed(1) || 0}%
Forecast (next 3 weeks): ${forecastLines}
Anomalies: ${anomalyDesc}
Day types: ${clusterDesc}
Tomorrow prediction: ${projPred}
Active projects: ${focusProjects}`;

  try {
    // Use the same LLM path as briefings
    const summary = await invoke('get_trends_ai_summary', { prompt });
    if (summary) {
      dom.trendsAiSummary.innerHTML = renderMarkdown(summary);
    } else {
      dom.trendsAiSummary.innerHTML = '<span class="briefing-disabled">Add an Anthropic API key in Settings to enable AI analysis.</span>';
    }
  } catch (err) {
    dom.trendsAiSummary.innerHTML = '<span class="briefing-disabled">AI summary unavailable. Add an Anthropic API key in Settings.</span>';
  }
}

function renderTrendsHeader(productivity, prediction) {
  const personal = isPersonalProfile();
  const trendIcon = productivity.trend === 'improving' ? '\u2197' : productivity.trend === 'declining' ? '\u2198' : '\u2192';
  dom.trendsPredictions.innerHTML = renderStatCards([
    { value: productivity.current_score, label: `${personal ? 'Momentum' : 'Productivity'} ${trendIcon}` },
    { value: productivity.baseline_avg, label: '12-wk Average' },
    { value: prediction.confidence.charAt(0).toUpperCase() + prediction.confidence.slice(1), label: personal ? 'Signal Confidence' : 'Forecast Confidence' },
    { value: productivity.trend.charAt(0).toUpperCase() + productivity.trend.slice(1), label: 'Trend' },
  ]);
}

function renderAnomalies(anomalies) {
  if (!anomalies || !anomalies.length) {
    dom.anomalyAlerts.innerHTML = '';
    return;
  }
  const kindLabels = { pr_merged: 'PRs Merged', issue_completed: 'Issues Done', commit_pushed: 'Commits', pr_reviewed: 'Reviews' };
  dom.anomalyAlerts.innerHTML = anomalies.slice(0, 6).map(a => {
    const icon = a.direction === 'high' ? '\u26a1' : '\u26a0';
    const label = kindLabels[a.kind] || a.kind;
    const desc = a.direction === 'high'
      ? `${label}: ${a.value} in ${a.week} (${a.z_score.toFixed(1)}\u03c3 above avg)`
      : `${label}: ${a.value} in ${a.week} (${Math.abs(a.z_score).toFixed(1)}\u03c3 below avg)`;
    return `<span class="anomaly-alert ${a.direction}">${icon} ${desc}</span>`;
  }).join('');
}

function renderForecastChart(velocity, prediction) {
  if (!hasChartJs()) return;
  if (state.charts.forecast) state.charts.forecast.destroy();

  const colorMap = {
    pr_merged: KIND_COLORS.merges,
    issue_completed: KIND_COLORS.completed || cssVar('--status-completed'),
    commit_pushed: KIND_COLORS.commits,
    pr_reviewed: KIND_COLORS.reviews,
  };
  const labelMap = { pr_merged: 'PRs Merged', issue_completed: 'Issues Done', commit_pushed: 'Commits', pr_reviewed: 'Reviews' };
  const allLabels = [...velocity.weeks, ...prediction.weeks_ahead].map(w => w.replace(/^\d{4}-/, ''));
  const datasets = [];

  for (const [kind, hist] of Object.entries(velocity.series)) {
    const color = colorMap[kind] || cssVar('--fallback');
    const forecast = prediction.forecasts[kind] || [];
    // Historical data (solid)
    datasets.push({
      label: labelMap[kind] || kind,
      data: [...hist, ...new Array(forecast.length).fill(null)],
      borderColor: color, backgroundColor: color + '33',
      fill: false, tension: 0.3, pointRadius: 3,
    });
    // Forecast (dashed)
    if (forecast.length) {
      datasets.push({
        label: `${labelMap[kind] || kind} forecast`,
        data: [...new Array(hist.length - 1).fill(null), hist[hist.length - 1], ...forecast],
        borderColor: color, borderDash: [6, 4],
        pointStyle: 'triangle', pointRadius: 4, fill: false,
      });
    }
  }

  state.charts.forecast = new Chart(dom.chartForecast.getContext('2d'), {
    type: 'line',
    data: { labels: allLabels, datasets },
    options: {
      ...TREND_CHART_OPTS,
      plugins: {
        ...TREND_CHART_OPTS.plugins,
        annotation: undefined,
      },
    },
  });
}

function renderProductivityChart(prod) {
  if (!hasChartJs()) return;
  if (state.charts.productivity) state.charts.productivity.destroy();

  const ctx = dom.chartProductivity.getContext('2d');
  state.charts.productivity = new Chart(ctx, {
    type: 'line',
    data: {
      labels: prod.weeks.map(w => w.replace(/^\d{4}-/, '')),
      datasets: [
        {
          label: 'Score',
          data: prod.scores,
          borderColor: cssVar('--highlight'),
          backgroundColor: cssVar('--highlight') + '26',
          fill: true, tension: 0.3, pointRadius: 3,
        },
        {
          label: 'Baseline',
          data: new Array(prod.scores.length).fill(prod.baseline_avg),
          borderColor: cssVar('--text-muted'), borderDash: [5, 5],
          pointRadius: 0, fill: false,
        },
      ],
    },
    options: TREND_CHART_OPTS,
  });
}

function renderDayClusters(data) {
  const palette = [cssVar('--highlight'), cssVar('--text-dim'), cssVar('--accent')];
  const dims = ['Commits', 'PRs', 'Reviews', 'Issues', 'Msgs'];
  let html = '<div class="cluster-grid">';
  data.clusters.forEach((c, i) => {
    const color = palette[i % palette.length];
    const maxC = Math.max(1, ...c.centroid);
    const bars = c.centroid.map((v, di) =>
      `<div class="cluster-bar-seg" style="flex:${Math.max(v, 0.1)};background:${color};opacity:${0.3 + 0.7 * (v / maxC)}" title="${dims[di]}: ${v}"></div>`
    ).join('');
    html += `<div class="cluster-card">
      <div class="cluster-dot" style="background:${color}"></div>
      <span class="cluster-name">${escapeHtml(c.name)}</span>
      <span class="cluster-count">${c.count} days</span>
      <div class="cluster-bar">${bars}</div>
    </div>`;
  });
  html += '</div>';
  dom.clusterContainer.innerHTML = html;
}

function renderProjectPrediction(predictions) {
  if (!predictions.length) {
    dom.projectPredContainer.innerHTML = '<div class="no-data">Not enough data</div>';
    return;
  }
  const maxProb = predictions[0]?.probability || 1;
  let html = '<div class="pred-list">';
  for (const p of predictions) {
    const pct = Math.round(p.probability * 100);
    const barPct = Math.round((p.probability / maxProb) * 100);
    html += `<div class="pred-row">
      <span class="pred-label">${escapeHtml(p.project)}</span>
      <div class="pred-bar-track">
        <div class="pred-bar" style="width:${barPct}%;background:var(--linear)">${pct}%</div>
      </div>
    </div>`;
  }
  html += '</div>';
  dom.projectPredContainer.innerHTML = html;
}

function renderVelocityChart(v) {
  if (!hasChartJs()) return;
  if (state.charts.velocity) state.charts.velocity.destroy();

  const colorMap = {
    pr_merged: KIND_COLORS.merges,
    issue_completed: KIND_COLORS.completed || cssVar('--status-completed'),
    commit_pushed: KIND_COLORS.commits,
    pr_reviewed: KIND_COLORS.reviews,
  };
  const labelMap = { pr_merged: 'PRs Merged', issue_completed: 'Issues Done', commit_pushed: 'Commits', pr_reviewed: 'Reviews' };
  const datasets = [];

  for (const [kind, values] of Object.entries(v.series)) {
    const color = colorMap[kind] || cssVar('--fallback');
    datasets.push({
      label: labelMap[kind] || kind,
      data: values,
      borderColor: color,
      backgroundColor: color + '33',
      fill: false, tension: 0.3, pointRadius: 3,
    });
    // Trend line
    const slope = v.trend_slopes[kind] || 0;
    const n = values.length;
    if (n >= 2) {
      const avg = values.reduce((a, b) => a + b, 0) / n;
      const midX = (n - 1) / 2;
      const trendData = values.map((_, i) => avg + slope * (i - midX));
      datasets.push({
        label: `${labelMap[kind] || kind} trend`,
        data: trendData,
        borderColor: color,
        borderDash: [5, 5],
        pointRadius: 0, fill: false,
      });
    }
  }

  const ctx = dom.chartVelocity.getContext('2d');
  state.charts.velocity = new Chart(ctx, {
    type: 'line',
    data: { labels: v.weeks.map(w => w.replace(/^\d{4}-/, '')), datasets },
    options: TREND_CHART_OPTS,
  });
}

function renderCycleTimeChart(cycleTime) {
  if (!hasChartJs()) return;
  if (state.charts.cycleTime) state.charts.cycleTime.destroy();
  if (!cycleTime || !cycleTime.length) {
    showNoData(dom.chartCycleTime, 'No cycle time data');
    return;
  }
  restoreCanvas(dom.chartCycleTime);

  const ctx = dom.chartCycleTime.getContext('2d');
  state.charts.cycleTime = new Chart(ctx, {
    type: 'bar',
    data: {
      labels: cycleTime.map(c => c.week.replace(/^\d{4}-/, '')),
      datasets: [{
        label: 'Avg Hours',
        data: cycleTime.map(c => Math.round(c.avg_hours * 10) / 10),
        backgroundColor: KIND_COLORS.issues || cssVar('--linear'),
        borderRadius: 3,
      }],
    },
    options: TREND_CHART_OPTS,
  });
}

function renderHeatmap(cells) {
  const max = Math.max(1, ...cells.map(c => c.count));
  const grid = Array.from({ length: 7 }, () => new Array(24).fill(0));
  for (const c of cells) grid[c.day][c.hour] = c.count;

  const days = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  let html = '<div class="heatmap-grid">';
  html += '<div class="heatmap-label"></div>';
  for (let h = 0; h < 24; h++) {
    html += `<div class="heatmap-hour-label">${h}</div>`;
  }
  for (let d = 0; d < 7; d++) {
    html += `<div class="heatmap-label">${days[d]}</div>`;
    for (let h = 0; h < 24; h++) {
      const count = grid[d][h];
      const intensity = count / max;
      const bg = `rgba(var(--heatmap), ${0.05 + intensity * 0.9})`;
      html += `<div class="heatmap-cell" data-dow="${d}" data-hour="${h}" style="background:${bg}" title="${days[d]} ${h}:00 — ${count} activities"></div>`;
    }
  }
  html += '</div>';
  dom.heatmapContainer.innerHTML = html;

  // Click handler for drill-down
  dom.heatmapContainer.querySelectorAll('.heatmap-cell').forEach(cell => {
    cell.addEventListener('click', () => {
      const dow = parseInt(cell.dataset.dow);
      const hour = parseInt(cell.dataset.hour);
      showHeatmapDetail(dow, hour, days[dow]);
    });
  });
}

async function showHeatmapDetail(dow, hour, dayName) {
  const overlay = dom.heatmapDetailOverlay;
  const title = document.getElementById('heatmap-detail-title');
  const body = document.getElementById('heatmap-detail-body');

  const hourStr = hour.toString().padStart(2, '0');
  title.textContent = `${dayName} ${hourStr}:00 — Activities (last 12 weeks)`;
  body.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-dim)">Loading...</div>';
  overlay.style.display = '';

  // Close handlers
  const closeBtn = document.getElementById('heatmap-detail-close');
  const close = () => { overlay.style.display = 'none'; };
  closeBtn.onclick = close;
  overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });

  try {
    const activities = await invoke('get_heatmap_activities', { dow, hour });

    if (!activities || activities.length === 0) {
      body.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-dim)">No activities in this slot</div>';
      return;
    }

    // Build counts by type
    const counts = {};
    for (const a of activities) {
      const kind = (a.kind || '').toLowerCase();
      const label = KIND_LABELS[kind] || formatKind(kind);
      counts[label] = (counts[label] || 0) + 1;
    }

    let html = '<div class="heatmap-detail-summary">';
    html += `<span class="kind-chip"><span class="chip-count">${activities.length}</span> total</span>`;
    for (const [label, count] of Object.entries(counts).sort((a, b) => b[1] - a[1])) {
      html += `<span class="kind-chip"><span class="chip-count">${count}</span> ${escapeHtml(label)}</span>`;
    }
    html += '</div>';

    html += `<table class="heatmap-detail-table">
      <thead><tr>
        <th>Type</th><th>Title</th><th>Project</th><th>Source</th><th>Date</th>
      </tr></thead><tbody>`;

    for (const a of activities) {
      const kind = (a.kind || '').toLowerCase();
      const kindClass = KIND_CLASSES[kind] || 'kind-default';
      const kindLabel = KIND_LABELS[kind] || formatKind(kind);
      const source = (a.source || '').toLowerCase();
      const sourceLabel = SOURCE_META[source]?.label || source;
      const titleText = escapeHtml(a.title || '');
      const project = a.project ? escapeHtml(a.project) : '—';
      const date = new Date(a.occurred_at).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });

      const titleHtml = a.url
        ? `<a class="activity-title-link" href="${escapeAttr(a.url)}" target="_blank" title="${escapeAttr(a.title)}">${titleText}</a>`
        : titleText;

      html += `<tr>
        <td><span class="kind-badge ${kindClass}">${kindLabel}</span></td>
        <td style="max-width:400px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${titleHtml}</td>
        <td><span class="project-tag">${project}</span></td>
        <td><span class="source-badge ${source}">${sourceLabel}</span></td>
        <td><span class="time-dim">${date}</span></td>
      </tr>`;
    }

    html += '</tbody></table>';
    body.innerHTML = html;
  } catch (err) {
    body.innerHTML = `<div style="padding:20px;text-align:center;color:var(--highlight)">Failed to load: ${escapeHtml(String(err))}</div>`;
  }
}

function renderFocusChart(focus) {
  if (!hasChartJs()) return;
  if (state.charts.focus) state.charts.focus.destroy();

  const palette = [cssVar('--highlight'), cssVar('--text-dim'), cssVar('--accent'), cssVar('--text'), cssVar('--status-closed'), cssVar('--status-review'), cssVar('--text-muted')];
  const datasets = [];
  let i = 0;
  for (const [proj, values] of Object.entries(focus.projects)) {
    const color = palette[i % palette.length];
    datasets.push({
      label: proj,
      data: values,
      backgroundColor: color + '88',
      borderColor: color,
      fill: 'origin',
      tension: 0.3, pointRadius: 0,
    });
    i++;
  }

  const ctx = dom.chartFocus.getContext('2d');
  state.charts.focus = new Chart(ctx, {
    type: 'line',
    data: { labels: focus.weeks.map(w => w.replace(/^\d{4}-/, '')), datasets },
    options: {
      ...TREND_CHART_OPTS,
      scales: {
        ...TREND_CHART_OPTS.scales,
        y: { ...TREND_CHART_OPTS.scales.y, stacked: true },
      },
    },
  });
}

function renderBurnoutChart(burnout) {
  if (!hasChartJs()) return;
  if (state.charts.burnout) state.charts.burnout.destroy();

  const ctx = dom.chartBurnout.getContext('2d');
  state.charts.burnout = new Chart(ctx, {
    type: 'line',
    data: {
      labels: burnout.weeks.map(w => w.replace(/^\d{4}-/, '')),
      datasets: [
        {
          label: 'Off-hours %',
          data: burnout.off_hours_pct.map(v => Math.round(v * 10) / 10),
          borderColor: cssVar('--status-closed'),
          backgroundColor: cssVar('--status-closed') + '1a',
          fill: true, tension: 0.3, yAxisID: 'y',
        },
        {
          label: 'Messages',
          data: burnout.message_volume,
          borderColor: cssVar('--status-open'),
          fill: false, tension: 0.3, yAxisID: 'y1',
        },
      ],
    },
    options: {
      ...TREND_CHART_OPTS,
      scales: {
        x: TREND_CHART_OPTS.scales.x,
        y: {
          ...TREND_CHART_OPTS.scales.y,
          position: 'left',
          title: { display: true, text: 'Off-hours %', color: cssVar('--text-dim'), font: { size: 10 } },
        },
        y1: {
          position: 'right',
          ticks: { color: cssVar('--text-dim'), font: { size: 11 } },
          grid: { drawOnChartArea: false },
          title: { display: true, text: 'Messages', color: cssVar('--text-dim'), font: { size: 10 } },
        },
      },
    },
  });
}

// ===== Utilities =====

/** Return '#fff' or '#000' depending on background luminance for readable contrast. */
function labelContrastColor(hexColor) {
  const hex = hexColor.replace(/^#/, '');
  const r = parseInt(hex.substring(0, 2), 16);
  const g = parseInt(hex.substring(2, 4), 16);
  const b = parseInt(hex.substring(4, 6), 16);
  // W3C relative luminance formula
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.5 ? '#000' : '#fff';
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
