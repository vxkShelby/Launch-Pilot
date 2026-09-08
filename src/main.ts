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

const STATUS_LABEL: Record<UpdateStatus, string> = {
  up_to_date: "Up to date",
  update_available: "Update available",
  unknown: "Unknown",
};

const LAUNCHER_LABEL: Record<string, string> = {
  steam: "Steam",
  epic: "Epic Games",
  gog: "GOG Galaxy",
};

function formatSize(bytes: number | null): string {
  if (bytes == null) return "—";
  const gb = bytes / 1024 ** 3;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

function buildGameRow(game: Game): HTMLElement {
  const row = document.createElement("div");
  row.className = "game-row";

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

async function loadGames() {
  const listEl = document.querySelector<HTMLElement>("#game-list");
  if (!listEl) return;

  let games: Game[];
  try {
    games = await invoke<Game[]>("list_games");
  } catch (err) {
    listEl.textContent = `Failed to list games: ${err}`;
    return;
  }

  if (games.length === 0) {
    listEl.textContent = "No games found (no supported launcher detected, or none installed).";
    return;
  }

  const byLauncher = new Map<string, Game[]>();
  for (const game of games) {
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

async function checkForUpdates() {
  const statusEl = document.querySelector<HTMLElement>("#update-status");
  if (!statusEl) return;

  try {
    const update = await check();
    if (!update) {
      statusEl.textContent = "Up to date.";
      return;
    }

    statusEl.textContent = `Update ${update.version} available, downloading...`;
    await update.downloadAndInstall();
    statusEl.textContent = "Update installed. Restarting...";
    await relaunch();
  } catch (err) {
    // ponytail: no retry/backoff — startup check only, next launch tries again
    statusEl.textContent = `Update check failed: ${err}`;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  checkForUpdates();
  loadGames();
});
