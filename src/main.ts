import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";

type UpdateStatus = "up_to_date" | "update_available" | "updating" | "unknown";

interface Game {
  launcher: string;
  id: string;
  name: string;
  installed_build: string | null;
  status: UpdateStatus;
  size_bytes: number | null;
  last_updated: number | null;
}

interface ProviderResult {
  id: string;
  name: string;
  games: Game[];
  running: boolean;
}

const STATUS_LABEL: Record<UpdateStatus, string> = {
  up_to_date: "Up to date",
  update_available: "Update available",
  updating: "Updating…",
  unknown: "Unknown",
};

const LAUNCHER_LABEL: Record<string, string> = {
  steam: "Steam",
  epic: "Epic Games",
  gog: "GOG Galaxy",
  ea: "EA app",
  ubisoft: "Ubisoft Connect",
  battlenet: "Battle.net",
  riot: "Riot Client",
  prismlauncher: "PrismLauncher",
  wargaming: "Wargaming Game Center",
  ageofthering: "Age of the Ring",
};

const REFRESH_INTERVAL_KEY = "lp.refreshIntervalMinutes";
const DEFAULT_REFRESH_MINUTES = 30;

let allGames: Game[] = [];
let runningLaunchers: Set<string> = new Set();
let refreshTimer: number | undefined;
let loadRequestId = 0;

function buildRunningDot(launcher: string): HTMLElement {
  const dot = document.createElement("span");
  dot.className = runningLaunchers.has(launcher) ? "running-dot running-dot-on" : "running-dot running-dot-off";
  dot.title = runningLaunchers.has(launcher) ? "Launcher is running" : "Launcher is not running";
  return dot;
}

// Real icon Windows itself associates with that launcher's own client exe
// (fetched via the launcher_icon command) — not a bundled/guessed logo.
// Undetected-on-this-machine launchers (no verified exe path) just show no
// icon rather than a placeholder pretending to be one.
const iconCache = new Map<string, string | null>();

function buildIcon(cacheKey: string, className: string, fetcher: () => Promise<string | null>): HTMLImageElement {
  const img = document.createElement("img");
  img.className = className;
  img.alt = "";
  img.hidden = true;
  const cached = iconCache.get(cacheKey);
  if (cached) {
    img.src = cached;
    img.hidden = false;
  } else if (!iconCache.has(cacheKey)) {
    fetcher()
      .catch(() => null)
      .then((uri) => {
        iconCache.set(cacheKey, uri);
        if (uri) {
          img.src = uri;
          img.hidden = false;
        }
      });
  }
  return img;
}

function buildLauncherIcon(launcher: string): HTMLImageElement {
  return buildIcon(launcher, "launcher-icon", () => invoke<string | null>("launcher_icon", { launcher }));
}

function buildGameIcon(game: Game): HTMLImageElement {
  const cacheKey = `${game.launcher}:${game.id}`;
  return buildIcon(cacheKey, "game-icon", () => invoke<string | null>("game_icon", { launcher: game.launcher, gameId: game.id }));
}

function formatSize(bytes: number | null): string {
  if (bytes == null) return "—";
  const gb = bytes / 1024 ** 3;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

function buildGameRow(game: Game): HTMLElement {
  const row = document.createElement("div");
  row.className = "game-row";
  row.dataset.name = game.name.toLowerCase();

  const status = document.createElement("span");
  status.className = `status status-${game.status}`;
  status.textContent = STATUS_LABEL[game.status];

  const nameWrap = document.createElement("span");
  nameWrap.className = "game-name";
  const name = document.createElement("span");
  name.className = "game-name-text";
  name.textContent = game.name;
  const size = document.createElement("span");
  size.className = "game-size";
  size.textContent = formatSize(game.size_bytes);
  nameWrap.append(buildGameIcon(game), name, size);

  row.append(status, nameWrap);

  if (game.status === "update_available" || game.status === "updating") {
    // No provider exposes a real "size of this pending update" field
    // (only the installed game's own size is known) — shown as "—"
    // rather than reusing size_bytes and implying a download size we
    // don't actually have.
    const updateSize = document.createElement("span");
    updateSize.className = "update-size";
    updateSize.textContent = formatSize(null);
    const btn = document.createElement("button");
    btn.className = "btn-update";
    btn.textContent = "Update";
    btn.onclick = () => invoke("trigger_update", { launcher: game.launcher, gameId: game.id });
    row.append(updateSize, btn);
  }

  return row;
}

function buildLauncherSection(launcher: string, games: Game[]): HTMLElement {
  const needsUpdate = games.filter((g) => g.status === "update_available" || g.status === "updating");
  const rest = games.filter((g) => g.status !== "update_available" && g.status !== "updating");

  const section = document.createElement("details");
  section.className = "launcher-section";
  section.dataset.launcher = launcher;

  const summary = document.createElement("summary");
  summary.appendChild(buildLauncherIcon(launcher));
  summary.appendChild(buildRunningDot(launcher));

  const title = document.createElement("span");
  title.className = "lane-title";
  title.textContent = LAUNCHER_LABEL[launcher] ?? launcher;
  summary.appendChild(title);

  if (needsUpdate.length > 0) {
    const badge = document.createElement("span");
    badge.className = "lane-badge";
    badge.textContent = `${needsUpdate.length} update${needsUpdate.length === 1 ? "" : "s"}`;
    summary.appendChild(badge);
  }

  const totalBytes = games.reduce<number | null>((sum, g) => (g.size_bytes == null ? sum : (sum ?? 0) + g.size_bytes), null);
  const meta = document.createElement("span");
  meta.className = "lane-meta";
  meta.textContent = `${games.length} game${games.length === 1 ? "" : "s"}${totalBytes == null ? "" : ` · ${formatSize(totalBytes)}`}`;
  summary.appendChild(meta);

  section.appendChild(summary);

  const body = document.createElement("div");
  body.className = "launcher-body";

  needsUpdate.forEach((g) => body.appendChild(buildGameRow(g)));

  if (rest.length > 0) {
    const restSection = document.createElement("details");
    restSection.className = "rest-section";
    const restSummary = document.createElement("summary");
    restSummary.textContent = `${rest.length} up to date / unknown`;
    restSection.appendChild(restSummary);
    const restBody = document.createElement("div");
    rest.forEach((g) => restBody.appendChild(buildGameRow(g)));
    restSection.appendChild(restBody);
    body.appendChild(restSection);
  }

  section.appendChild(body);
  return section;
}

function updateStats() {
  const statsEl = document.querySelector<HTMLElement>("#stats");
  const updateAllBtn = document.querySelector<HTMLButtonElement>("#update-all-btn");
  if (!statsEl) return;

  const pending = allGames.filter((g) => g.status === "update_available").length;
  statsEl.textContent = `${allGames.length} games · ${pending} update${pending === 1 ? "" : "s"} pending`;
  statsEl.classList.toggle("stats-pending", pending > 0);

  if (updateAllBtn) {
    updateAllBtn.hidden = pending === 0;
    updateAllBtn.textContent = `Update all (${pending})`;
  }
}

function setLastChecked() {
  const el = document.querySelector<HTMLElement>("#last-checked");
  if (el) el.textContent = `Last checked ${new Date().toLocaleTimeString()}`;
}

function buildLoadingPlaceholder(launcher: string): HTMLElement {
  const el = document.createElement("div");
  el.className = "launcher-loading muted";
  el.dataset.launcher = launcher;
  el.textContent = `${LAUNCHER_LABEL[launcher] ?? launcher} — loading…`;
  return el;
}

function buildConnectedRow(result: ProviderResult): HTMLElement {
  const row = document.createElement("div");
  row.className = "connected-row";
  row.dataset.launcher = result.id;
  const name = document.createElement("span");
  name.appendChild(buildLauncherIcon(result.id));
  name.appendChild(buildRunningDot(result.id));
  name.append(LAUNCHER_LABEL[result.id] ?? result.name);
  const btn = document.createElement("button");
  btn.textContent = "Open";
  btn.onclick = () => invoke("trigger_update", { launcher: result.id, gameId: "" });
  row.append(name, btn);
  return row;
}

function ensureConnectedSection(listEl: HTMLElement): HTMLElement {
  let section = listEl.querySelector<HTMLElement>(".connected-only");
  if (!section) {
    section = document.createElement("div");
    section.className = "connected-only";
    const heading = document.createElement("p");
    heading.className = "muted";
    heading.textContent = "Connected (no game list available):";
    section.appendChild(heading);
    listEl.appendChild(section);
  }
  return section;
}

// Loads each launcher independently and renders it the moment it resolves,
// instead of one big call that makes every launcher wait behind whichever
// provider is slowest (a big Steam library, GOG's network round-trip).
async function loadGames() {
  const listEl = document.querySelector<HTMLElement>("#game-list");
  if (!listEl) return;
  const requestId = ++loadRequestId;

  let ids: string[];
  try {
    ids = await invoke<string[]>("launcher_ids");
  } catch (err) {
    listEl.textContent = `Failed to list launchers: ${err}`;
    return;
  }
  if (requestId !== loadRequestId) return;

  allGames = [];
  runningLaunchers = new Set();
  listEl.innerHTML = "";
  updateStats();

  const placeholders = new Map<string, HTMLElement>();
  for (const id of ids) {
    const placeholder = buildLoadingPlaceholder(id);
    placeholders.set(id, placeholder);
    listEl.appendChild(placeholder);
  }

  let settled = 0;
  ids.forEach((id) => {
    invoke<ProviderResult | null>("provider_data", { launcher: id })
      .catch(() => null)
      .then((result) => {
        if (requestId !== loadRequestId) return;
        settled += 1;

        const placeholder = placeholders.get(id);
        if (result && result.running) runningLaunchers.add(id);

        if (!result) {
          placeholder?.remove();
        } else if (result.games.length === 0) {
          placeholder?.remove();
          ensureConnectedSection(listEl).appendChild(buildConnectedRow(result));
        } else {
          const games = [...result.games].sort((a, b) => a.name.localeCompare(b.name));
          allGames.push(...games);
          placeholder?.replaceWith(buildLauncherSection(id, games));
        }

        updateStats();
        setLastChecked();
        applySearchFilter();

        if (settled === ids.length && allGames.length === 0 && listEl.querySelector(".connected-only") === null) {
          listEl.textContent = "No games found (no supported launcher detected, or none installed).";
        }
      });
  });
}

function applySearchFilter() {
  const search = document.querySelector<HTMLInputElement>("#search");
  const query = search?.value.trim().toLowerCase() ?? "";

  document.querySelectorAll<HTMLDetailsElement>(".launcher-section").forEach((section) => {
    let sectionHasMatch = false;
    section.querySelectorAll<HTMLElement>(".game-row").forEach((row) => {
      const matches = query === "" || (row.dataset.name ?? "").includes(query);
      row.hidden = !matches;
      if (matches) sectionHasMatch = true;
    });
    section.hidden = query !== "" && !sectionHasMatch;
    if (query !== "" && sectionHasMatch) {
      section.open = true;
      section.querySelectorAll<HTMLDetailsElement>(".rest-section").forEach((r) => (r.open = true));
    }
  });
}

async function updateAll() {
  const btn = document.querySelector<HTMLButtonElement>("#update-all-btn");
  const pending = allGames.filter((g) => g.status === "update_available");
  if (pending.length === 0 || !btn) return;

  btn.disabled = true;
  for (let i = 0; i < pending.length; i++) {
    const game = pending[i];
    btn.textContent = `Updating ${i + 1}/${pending.length}…`;
    try {
      await invoke("trigger_update", { launcher: game.launcher, gameId: game.id });
    } catch {
      // ponytail: best-effort — a failed trigger for one game shouldn't stop the rest
    }
    await new Promise((r) => setTimeout(r, 400));
  }
  btn.disabled = false;
  updateStats();
}

function getRefreshMinutes(): number {
  const stored = localStorage.getItem(REFRESH_INTERVAL_KEY);
  const n = stored ? parseInt(stored, 10) : DEFAULT_REFRESH_MINUTES;
  return Number.isFinite(n) ? n : DEFAULT_REFRESH_MINUTES;
}

function applyRefreshInterval() {
  if (refreshTimer) window.clearInterval(refreshTimer);
  const minutes = getRefreshMinutes();
  if (minutes <= 0) return;
  refreshTimer = window.setInterval(() => {
    loadGames();
    checkForUpdates();
  }, minutes * 60 * 1000);
}

async function checkForUpdates() {
  const statusEl = document.querySelector<HTMLElement>("#update-status");
  if (!statusEl) return;

  try {
    const update = await check();
    if (!update) {
      statusEl.textContent = "";
      return;
    }

    statusEl.textContent = `Update ${update.version} available, downloading...`;
    await update.downloadAndInstall();
    statusEl.textContent = "Update installed. Restarting...";
    await relaunch();
  } catch (err) {
    // ponytail: no retry/backoff — next scheduled check (or launch) tries again
    statusEl.textContent = `Update check failed: ${err}`;
  }
}

function setupControls() {
  document.querySelector("#search")?.addEventListener("input", applySearchFilter);
  document.querySelector("#refresh-btn")?.addEventListener("click", () => loadGames());
  document.querySelector("#update-all-btn")?.addEventListener("click", () => updateAll());

  const panel = document.querySelector<HTMLElement>("#settings-panel");
  document.querySelector("#settings-btn")?.addEventListener("click", () => {
    if (panel) panel.hidden = false;
  });
  document.querySelector("#settings-close")?.addEventListener("click", () => {
    if (panel) panel.hidden = true;
  });

  const intervalSelect = document.querySelector<HTMLSelectElement>("#interval-select");
  if (intervalSelect) {
    intervalSelect.value = String(getRefreshMinutes());
    intervalSelect.addEventListener("change", () => {
      localStorage.setItem(REFRESH_INTERVAL_KEY, intervalSelect.value);
      applyRefreshInterval();
    });
  }
}

window.addEventListener("DOMContentLoaded", () => {
  setupControls();
  checkForUpdates();
  loadGames();
  applyRefreshInterval();
});
