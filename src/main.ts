import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";

type UpdateStatus = "up_to_date" | "update_available" | "unknown";

interface Game {
  launcher: string;
  id: string;
  name: string;
  installed_build: string | null;
  status: UpdateStatus;
  size_bytes: number | null;
  last_updated: number | null;
}

interface LauncherInfo {
  id: string;
  name: string;
}

interface DashboardData {
  games: Game[];
  connected_only: LauncherInfo[];
}

const STATUS_LABEL: Record<UpdateStatus, string> = {
  up_to_date: "Up to date",
  update_available: "Update available",
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
};

const REFRESH_INTERVAL_KEY = "lp.refreshIntervalMinutes";
const DEFAULT_REFRESH_MINUTES = 30;

let allGames: Game[] = [];
let refreshTimer: number | undefined;

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

  const name = document.createElement("span");
  name.className = "game-name";
  name.textContent = game.name;

  const size = document.createElement("span");
  size.className = "game-size";
  size.textContent = formatSize(game.size_bytes);

  row.append(status, name, size);

  if (game.status === "update_available") {
    const btn = document.createElement("button");
    btn.textContent = "Update";
    btn.onclick = () => invoke("trigger_update", { launcher: game.launcher, gameId: game.id });
    row.append(btn);
  }

  return row;
}

function buildLauncherSection(launcher: string, games: Game[]): HTMLElement {
  const needsUpdate = games.filter((g) => g.status === "update_available");
  const rest = games.filter((g) => g.status !== "update_available");

  const section = document.createElement("details");
  section.className = "launcher-section";
  section.dataset.launcher = launcher;

  const summary = document.createElement("summary");
  const badge = needsUpdate.length > 0 ? ` (${needsUpdate.length} update${needsUpdate.length === 1 ? "" : "s"})` : "";
  summary.textContent = `${LAUNCHER_LABEL[launcher] ?? launcher} — ${games.length} game${games.length === 1 ? "" : "s"}${badge}`;
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

  if (updateAllBtn) {
    updateAllBtn.hidden = pending === 0;
    updateAllBtn.textContent = `Update all (${pending})`;
  }
}

function setLastChecked() {
  const el = document.querySelector<HTMLElement>("#last-checked");
  if (el) el.textContent = `Last checked ${new Date().toLocaleTimeString()}`;
}

async function loadGames() {
  const listEl = document.querySelector<HTMLElement>("#game-list");
  if (!listEl) return;

  let data: DashboardData;
  try {
    data = await invoke<DashboardData>("dashboard_data");
  } catch (err) {
    listEl.textContent = `Failed to list games: ${err}`;
    return;
  }
  allGames = data.games;

  updateStats();
  setLastChecked();

  if (allGames.length === 0) {
    listEl.textContent = "No games found (no supported launcher detected, or none installed).";
  } else {
    const byLauncher = new Map<string, Game[]>();
    for (const game of allGames) {
      const list = byLauncher.get(game.launcher) ?? [];
      list.push(game);
      byLauncher.set(game.launcher, list);
    }

    listEl.innerHTML = "";
    for (const [launcher, launcherGames] of byLauncher) {
      launcherGames.sort((a, b) => a.name.localeCompare(b.name));
      listEl.appendChild(buildLauncherSection(launcher, launcherGames));
    }
  }

  renderConnectedOnly(listEl, data.connected_only);
  applySearchFilter();
}

function renderConnectedOnly(listEl: HTMLElement, launchers: LauncherInfo[]) {
  if (launchers.length === 0) return;

  const section = document.createElement("div");
  section.className = "connected-only";
  const heading = document.createElement("p");
  heading.className = "muted";
  heading.textContent = "Connected (no game list available):";
  section.appendChild(heading);

  launchers.forEach((launcher) => {
    const row = document.createElement("div");
    row.className = "connected-row";
    const name = document.createElement("span");
    name.textContent = LAUNCHER_LABEL[launcher.id] ?? launcher.name;
    const btn = document.createElement("button");
    btn.textContent = "Open";
    btn.onclick = () => invoke("trigger_update", { launcher: launcher.id, gameId: "" });
    row.append(name, btn);
    section.appendChild(row);
  });

  listEl.appendChild(section);
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
