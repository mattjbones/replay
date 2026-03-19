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
  authStatus: { github: false, linear: false, slack: false, notion: false },
  digest: null,
  llmSummaryCache: {},
  llmOpen: false,
};

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
  slack: {
    label: "Slack",
    color: "#4a154b",
    helper: 'Create app from <a href="#" onclick="return false">manifest</a>, then paste xoxp token.',
    placeholder: "xoxp-...",
  },
  notion: {
    label: "Notion",
    color: "#ffffff",
    helper: 'Create integration at <a href="https://www.notion.so/my-integrations" target="_blank">notion.so/my-integrations</a>',
    placeholder: "ntn_...",
  },
};

const KIND_ICONS = {
  pr: "PR",
  pull_request: "PR",
  commit: "C",
  review: "R",
  issue: "I",
  comment: "💬",
  message: "M",
  page: "P",
};

const KIND_CLASSES = {
  pr: "kind-pr",
  pull_request: "kind-pr",
  commit: "kind-commit",
  review: "kind-review",
  issue: "kind-issue",
  comment: "kind-comment",
  message: "kind-message",
  page: "kind-page",
};

// ===== DOM References =====

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => document.querySelectorAll(sel);

const dom = {
  headerDate: $("#header-date"),
  tabBar: $("#tab-bar"),
  authSection: $("#auth-section"),
  authList: $("#auth-list"),
  statsBar: $("#stats-bar"),
  statTotal: $("#stat-total"),
  statBadges: $("#stat-badges"),
  activitySection: $("#activity-section"),
  activityList: $("#activity-list"),
  llmSection: $("#llm-section"),
  llmToggle: $("#llm-toggle"),
  llmContent: $("#llm-content"),
  llmText: $("#llm-text"),
  syncBtn: $("#sync-btn"),
  loading: $("#loading"),
  emptyState: $("#empty-state"),
  toast: $("#toast"),
};

// ===== Initialization =====

document.addEventListener("DOMContentLoaded", async () => {
  renderDate();
  bindEvents();
  await refreshAuthStatus();
  await loadDigest();
});

function bindEvents() {
  // Tab switching
  dom.tabBar.addEventListener("click", (e) => {
    const tab = e.target.closest(".tab");
    if (!tab || tab.classList.contains("active")) return;
    $$(".tab").forEach((t) => t.classList.remove("active"));
    tab.classList.add("active");
    state.period = tab.dataset.period;
    state.llmOpen = false;
    closeLlm();
    loadDigest();
  });

  // Sync button
  dom.syncBtn.addEventListener("click", handleSync);

  // LLM toggle
  dom.llmToggle.addEventListener("click", toggleLlm);
}

// ===== Date Display =====

function renderDate() {
  const now = new Date();
  const opts = { weekday: "long", month: "long", day: "numeric", year: "numeric" };
  dom.headerDate.textContent = now.toLocaleDateString("en-US", opts);
}

// ===== Auth =====

async function refreshAuthStatus() {
  try {
    state.authStatus = await invoke("get_auth_status");
  } catch {
    state.authStatus = { github: false, linear: false, slack: false, notion: false };
  }
  renderAuth();
}

function renderAuth() {
  const anyDisconnected = Object.values(state.authStatus).some((v) => !v);
  dom.authSection.style.display = anyDisconnected ? "" : "none";
  if (!anyDisconnected) return;

  let html = "";
  for (const [source, meta] of Object.entries(SOURCE_META)) {
    const connected = state.authStatus[source];
    html += `
      <div class="auth-source">
        <div class="auth-dot ${connected ? "connected" : "disconnected"}"></div>
        <div class="auth-info">
          <div class="auth-name">${meta.label}</div>
          ${connected
            ? '<div class="auth-helper" style="color:#34d058;">Connected</div>'
            : `<div class="auth-helper">${meta.helper}</div>
               <div class="auth-input-row">
                 <input type="password" class="auth-input" id="token-${source}"
                   placeholder="${meta.placeholder}" autocomplete="off" spellcheck="false">
                 <button class="btn btn-small btn-highlight" onclick="saveToken('${source}')">Save</button>
               </div>`
          }
        </div>
      </div>`;
  }
  dom.authList.innerHTML = html;
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
    showToast(`${SOURCE_META[source].label} connected!`, "success");
    await refreshAuthStatus();
    await loadDigest();
  } catch (err) {
    showToast(`Failed to save token: ${err}`, "error");
  }
}

// Make saveToken available globally for onclick handlers
window.saveToken = saveToken;

// ===== Digest Loading =====

async function loadDigest() {
  showLoading(true);
  hideContent();

  try {
    const args = { period: state.period };
    if (state.date) args.date = state.date;
    state.digest = await invoke("get_digest", args);
    renderDigest();
  } catch (err) {
    showToast(`Failed to load digest: ${err}`, "error");
    showLoading(false);
    dom.emptyState.style.display = "";
  }
}

function renderDigest() {
  showLoading(false);
  const d = state.digest;

  if (!d || !d.activities || d.activities.length === 0) {
    dom.emptyState.style.display = "";
    dom.statsBar.style.display = "none";
    dom.activitySection.style.display = "none";
    dom.llmSection.style.display = "none";
    return;
  }

  renderStats(d.stats);
  renderActivities(d.activities);
  dom.llmSection.style.display = "";
}

// ===== Stats =====

function renderStats(stats) {
  if (!stats) return;
  dom.statsBar.style.display = "";
  dom.statTotal.textContent = `${stats.total_activities} activit${stats.total_activities === 1 ? "y" : "ies"}`;

  let badges = "";
  if (stats.by_source) {
    for (const [source, count] of Object.entries(stats.by_source)) {
      if (count > 0) {
        badges += `<span class="stat-badge ${source}">${SOURCE_META[source]?.label || source} ${count}</span>`;
      }
    }
  }
  dom.statBadges.innerHTML = badges;
}

// ===== Activities =====

function renderActivities(activities) {
  dom.activitySection.style.display = "";

  // Group by source
  const groups = {};
  for (const a of activities) {
    const src = a.source || "other";
    if (!groups[src]) groups[src] = [];
    groups[src].push(a);
  }

  // Sort each group by occurred_at descending
  for (const arr of Object.values(groups)) {
    arr.sort((a, b) => new Date(b.occurred_at) - new Date(a.occurred_at));
  }

  const sourceOrder = ["github", "linear", "slack", "notion"];
  const sortedKeys = Object.keys(groups).sort(
    (a, b) => (sourceOrder.indexOf(a) === -1 ? 99 : sourceOrder.indexOf(a)) -
              (sourceOrder.indexOf(b) === -1 ? 99 : sourceOrder.indexOf(b))
  );

  let html = "";
  for (const source of sortedKeys) {
    const items = groups[source];
    const label = SOURCE_META[source]?.label || source;
    html += `<div class="activity-group">`;
    html += `<div class="group-header">${label}</div>`;
    for (const item of items) {
      html += renderActivityItem(item);
    }
    html += `</div>`;
  }

  dom.activityList.innerHTML = html;
}

function renderActivityItem(a) {
  const kind = (a.kind || "").toLowerCase();
  const kindClass = KIND_CLASSES[kind] || "kind-default";
  const kindIcon = KIND_ICONS[kind] || kind.charAt(0).toUpperCase() || "?";
  const relTime = relativeTime(a.occurred_at);
  const projectHtml = a.project
    ? `<span class="project-tag">${escapeHtml(a.project)}</span>`
    : "";

  const titleHtml = a.url
    ? `<a class="activity-title" href="${escapeHtml(a.url)}" target="_blank" title="${escapeHtml(a.title)}">${escapeHtml(a.title)}</a>`
    : `<span class="activity-title" title="${escapeHtml(a.title)}">${escapeHtml(a.title)}</span>`;

  return `
    <div class="activity-item">
      <span class="activity-kind ${kindClass}">${kindIcon}</span>
      <div class="activity-body">
        ${titleHtml}
        <div class="activity-meta">
          ${projectHtml}
          <span>${relTime}</span>
        </div>
      </div>
    </div>`;
}

// ===== LLM Summary =====

function toggleLlm() {
  state.llmOpen = !state.llmOpen;
  const chevron = $(".llm-chevron");

  if (state.llmOpen) {
    chevron.classList.add("open");
    dom.llmContent.style.display = "";
    fetchLlmSummary();
  } else {
    closeLlm();
  }
}

function closeLlm() {
  state.llmOpen = false;
  const chevron = $(".llm-chevron");
  chevron?.classList.remove("open");
  if (dom.llmContent) dom.llmContent.style.display = "none";
}

async function fetchLlmSummary() {
  const cacheKey = `${state.period}:${state.date || "now"}`;

  if (state.llmSummaryCache[cacheKey] !== undefined) {
    renderLlmText(state.llmSummaryCache[cacheKey]);
    return;
  }

  dom.llmText.innerHTML = `<div class="llm-loading"><span></span><span></span><span></span></div>`;

  try {
    const args = { period: state.period };
    if (state.date) args.date = state.date;
    const summary = await invoke("get_llm_summary", args);
    state.llmSummaryCache[cacheKey] = summary;
    renderLlmText(summary);
  } catch (err) {
    renderLlmText(null);
  }
}

function renderLlmText(summary) {
  if (summary) {
    dom.llmText.textContent = summary;
  } else {
    dom.llmText.innerHTML = `<span style="color:var(--text-muted)">LLM not enabled. Set <code>llm.enabled = true</code> in config.</span>`;
  }
}

// ===== Sync =====

async function handleSync() {
  dom.syncBtn.disabled = true;
  dom.syncBtn.classList.add("syncing");

  try {
    const msg = await invoke("trigger_sync");
    showToast(msg || "Sync complete", "success");
    await loadDigest();
  } catch (err) {
    showToast(`Sync failed: ${err}`, "error");
  } finally {
    dom.syncBtn.disabled = false;
    dom.syncBtn.classList.remove("syncing");
  }
}

// ===== UI Helpers =====

function showLoading(show) {
  dom.loading.style.display = show ? "" : "none";
}

function hideContent() {
  dom.statsBar.style.display = "none";
  dom.activitySection.style.display = "none";
  dom.llmSection.style.display = "none";
  dom.emptyState.style.display = "none";
}

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

function relativeTime(dateStr) {
  if (!dateStr) return "";
  const now = Date.now();
  const then = new Date(dateStr).getTime();
  const diffMs = now - then;
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHr = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHr / 24);

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
