import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type UpdateStatus = "up_to_date" | "update_available" | "updating" | "unknown";

// Every field below mirrors the backend JSON (snake_case). `null` always means
// "no verified source" and is rendered as nothing, never as 0 / "never".
interface Game {
  launcher: string;
  id: string;
  name: string;
  installed_build: string | null;
  status: UpdateStatus;
  size_bytes: number | null;
  last_updated: number | null;
  last_played: number | null;
  install_date: string | null;
  update_bytes_total: number | null;
  update_bytes_done: number | null;
  update_scheduled: number | null;
}

interface ProviderResult {
  id: string;
  name: string;
  games: Game[];
  running: boolean;
  can_launch: boolean;
  can_update: boolean;
  error: string | null;
}

// One entry per launcher id. "absent" = provider_data resolved null (not installed).
type LaneState =
  | { kind: "loading" }
  | { kind: "absent" }
  | { kind: "ok"; result: ProviderResult }
  | { kind: "error"; name: string; message: string; retrying: boolean };

type StatusFilter = "all" | "pending" | "up_to_date" | "unknown";
type SortMode = "name" | "status" | "last_played" | "size";
type Theme = "system" | "dark" | "light";

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
  rockstar: "Rockstar Games Launcher",
  prismlauncher: "PrismLauncher",
  wargaming: "Wargaming Game Center",
  ageofthering: "Age of the Ring",
};

// Backend contract: EA's size_bytes comes from the installer-written
// Uninstall EstimatedSize, so it is approximate. Other launchers' sizes are not flagged.
const APPROX_SIZE_LAUNCHERS = new Set(["ea"]);

const REFRESH_INTERVAL_KEY = "lp.refreshIntervalMinutes";
const DEFAULT_REFRESH_MINUTES = 30;
const ONBOARDING_SEEN_KEY = "lp.onboardingSeen";
const THEME_KEY = "lp.theme";
const SORT_KEY = "lp.sort";
const IGNORED_KEY = "lp.ignored";

// ---------------------------------------------------------------- storage
// localStorage can throw or come back empty (private window, blocked data).
const store = {
  get(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* the setting just won't persist */
    }
  },
};

// ------------------------------------------------------------------ state
const lanes = new Map<string, LaneState>();
const laneEls = new Map<string, HTMLElement>();
let launcherIds: string[] = [];
let loadRequestId = 0;
let loadingNow = false;
let updateAllRunning = false;
let refreshTimer: number | undefined;

let statusFilter: StatusFilter = "all";
let sortMode: SortMode = "name";

// What the user opened/closed by hand. Absent = default (auto-open lanes with updates).
const openState = new Map<string, boolean>();
const restOpenState = new Map<string, boolean>();

// key `${launcher}:${gameId}` -> display name, for the Settings restore list.
const ignored = new Map<string, string>();

const $ = <T extends HTMLElement>(sel: string) => document.querySelector<T>(sel);

function gameKey(g: { launcher: string; id: string }): string {
  return `${g.launcher}:${g.id}`;
}

function launcherLabel(id: string, fallback?: string): string {
  return LAUNCHER_LABEL[id] ?? fallback ?? id;
}

function loadIgnored() {
  try {
    const raw = store.get(IGNORED_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    if (parsed && typeof parsed === "object") {
      for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
        if (typeof v === "string") ignored.set(k, v);
      }
    }
  } catch {
    /* corrupt value: start empty */
  }
}

function saveIgnored() {
  store.set(IGNORED_KEY, JSON.stringify(Object.fromEntries(ignored)));
}

// ------------------------------------------------------------------ theme
function applyTheme(theme: Theme) {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  // Make the OS title bar follow the in-app choice (null = follow Windows).
  // Needs the core:window:allow-set-theme capability.
  try {
    getCurrentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => {});
  } catch {
    /* not running inside Tauri (plain browser): the CSS theme still applies */
  }
}

function getTheme(): Theme {
  const t = store.get(THEME_KEY);
  return t === "dark" || t === "light" ? t : "system";
}

applyTheme(getTheme());

// --------------------------------------------------------------- formatting
// One size formatter for every place a size is shown (B up to TB).
function formatSize(bytes: number | null): string {
  if (bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let u = 0;
  while (value >= 1024 && u < units.length - 1) {
    value /= 1024;
    u += 1;
  }
  // Rounding must not produce "1024.0 GB": bump the unit instead.
  const decimals = u >= 3 ? 1 : 0;
  if (u < units.length - 1 && Number(value.toFixed(decimals)) >= 1024) {
    value /= 1024;
    u += 1;
  }
  return `${u >= 3 ? value.toFixed(1) : value.toFixed(0)} ${units[u]}`;
}

function formatDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

function formatDateTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

// install_date is "YYYY-MM-DD HH:MM:SS" in machine-local time with no timezone,
// so it is shown as-is (date part) and never fed through `new Date()`.
function formatInstallDate(s: string): string {
  const m = /^\d{4}-\d{2}-\d{2}/.exec(s);
  return m ? m[0] : s;
}

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

// ------------------------------------------------------------ pending logic
// The single definition of "pending", used by the lane badge, the header
// stats, the lane ordering and the Update-all button.
interface PendingCounts {
  available: number;
  updating: number;
}

function countPending(games: Game[]): PendingCounts {
  let available = 0;
  let updating = 0;
  for (const g of games) {
    if (g.status === "update_available") available += 1;
    else if (g.status === "updating") updating += 1;
  }
  return { available, updating };
}

function pendingLabel(c: PendingCounts): string {
  const parts: string[] = [];
  if (c.available > 0) parts.push(plural(c.available, "update"));
  if (c.updating > 0) parts.push(`${c.updating} updating`);
  return parts.join(" · ");
}

function isPending(g: Game): boolean {
  return g.status === "update_available" || g.status === "updating";
}

function visibleGamesOf(result: ProviderResult): Game[] {
  return result.games.filter((g) => !ignored.has(gameKey(g)));
}

function allVisibleGames(): Game[] {
  const out: Game[] = [];
  for (const state of lanes.values()) {
    if (state.kind === "ok") out.push(...visibleGamesOf(state.result));
  }
  return out;
}

function allGames(): Game[] {
  const out: Game[] = [];
  for (const state of lanes.values()) {
    if (state.kind === "ok") out.push(...state.result.games);
  }
  return out;
}

// ---------------------------------------------------------------- sorting
const STATUS_RANK: Record<UpdateStatus, number> = { update_available: 0, updating: 1, unknown: 2, up_to_date: 3 };

function cmpNullLast(a: number | null, b: number | null, desc: boolean): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  return desc ? b - a : a - b;
}

function compareGames(a: Game, b: Game): number {
  let r = 0;
  if (sortMode === "status") r = STATUS_RANK[a.status] - STATUS_RANK[b.status];
  else if (sortMode === "last_played") r = cmpNullLast(a.last_played, b.last_played, true);
  else if (sortMode === "size") r = cmpNullLast(a.size_bytes, b.size_bytes, true);
  return r !== 0 ? r : a.name.localeCompare(b.name);
}

// --------------------------------------------------------------- icons (H2)
// Icons are requested only when their <img> becomes visible (rows inside a
// closed <details> are never rendered, so they are never requested), requests
// are de-duplicated while in flight, and results are cached per id.
const iconData = new Map<string, string | null>();
const iconPromises = new Map<string, Promise<string | null>>();
const iconFetchers = new WeakMap<HTMLImageElement, { key: string; fetch: () => Promise<string | null> }>();
let iconObserver: IntersectionObserver | null = null;

function requestIcon(key: string, fetcher: () => Promise<string | null>): Promise<string | null> {
  let p = iconPromises.get(key);
  if (!p) {
    p = fetcher().then(
      (uri) => {
        iconData.set(key, uri);
        return uri;
      },
      () => {
        // A failed call is not remembered as "no icon": the next sighting retries.
        iconPromises.delete(key);
        return null;
      },
    );
    iconPromises.set(key, p);
  }
  return p;
}

function getIconObserver(): IntersectionObserver {
  if (!iconObserver) {
    iconObserver = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (!e.isIntersecting) continue;
          const img = e.target as HTMLImageElement;
          iconObserver?.unobserve(img);
          const job = iconFetchers.get(img);
          if (!job) continue;
          void requestIcon(job.key, job.fetch).then((uri) => {
            if (uri) img.src = uri;
          });
        }
      },
      { root: $("#container"), rootMargin: "200px" },
    );
  }
  return iconObserver;
}

function buildIcon(key: string, className: string, fetcher: () => Promise<string | null>): HTMLImageElement {
  const img = document.createElement("img");
  img.className = className;
  img.alt = "";
  const cached = iconData.get(key);
  if (cached) {
    img.src = cached;
  } else if (!iconData.has(key)) {
    iconFetchers.set(img, { key, fetch: fetcher });
    getIconObserver().observe(img);
  }
  return img;
}

function buildLauncherIcon(launcher: string): HTMLImageElement {
  return buildIcon(`launcher:${launcher}`, "launcher-icon", () => invoke<string | null>("launcher_icon", { launcher }));
}

function buildGameIcon(game: Game): HTMLImageElement {
  return buildIcon(gameKey(game), "game-icon", () => invoke<string | null>("game_icon", { launcher: game.launcher, gameId: game.id }));
}

// --------------------------------------------------------- focus / messages
// Re-rendering a lane replaces its DOM; keep keyboard focus on "the same" control.
function withFocusKept(fn: () => void) {
  const active = document.activeElement as HTMLElement | null;
  const focusId = active?.dataset?.focusId;
  fn();
  if (focusId) {
    document.querySelector<HTMLElement>(`[data-focus-id="${CSS.escape(focusId)}"]`)?.focus({ preventScroll: true });
  }
}

function showRowMsg(row: HTMLElement, text: string, isError: boolean) {
  row.querySelector(".row-msg")?.remove();
  const msg = document.createElement("div");
  msg.className = isError ? "row-msg row-msg-error" : "row-msg";
  msg.setAttribute("role", isError ? "alert" : "status");
  msg.textContent = text;
  row.append(msg);
  window.setTimeout(() => {
    if (msg.isConnected) msg.remove();
  }, isError ? 12000 : 4000);
}

let actionTimer: number | undefined;
function setAction(text: string, autoClearMs = 0) {
  const el = $("#action-status");
  if (!el) return;
  el.textContent = text;
  el.hidden = text === "";
  window.clearTimeout(actionTimer);
  if (text && autoClearMs > 0) actionTimer = window.setTimeout(() => setAction(""), autoClearMs);
}

function announce(text: string) {
  const el = $("#sr-status");
  if (el) el.textContent = text;
}

// -------------------------------------------------------------- game rows
function buildRunningDot(running: boolean): HTMLElement {
  const dot = document.createElement("span");
  dot.className = running ? "running-dot running-dot-on" : "running-dot running-dot-off";
  dot.setAttribute("role", "img");
  const label = running ? "Launcher is running" : "Launcher is not running";
  dot.setAttribute("aria-label", label);
  dot.title = label;
  return dot;
}

function buildSubLine(game: Game, result: ProviderResult): HTMLElement | null {
  const parts: Array<string | HTMLElement> = [];

  if (isPending(game)) {
    const total = game.update_bytes_total;
    const done = game.update_bytes_done;
    if (game.status === "updating" && total != null && done != null) {
      const pct = total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : 0;
      const wrap = document.createElement("span");
      wrap.className = "sub-progress";
      const bar = document.createElement("progress");
      bar.max = total;
      bar.value = Math.min(done, total);
      bar.setAttribute("aria-label", `Download progress for ${game.name}`);
      wrap.append(bar, ` ${formatSize(done)} of ${formatSize(total)} (${pct}%)`);
      parts.push(wrap);
    } else if (total != null) {
      parts.push(`${formatSize(total)} to download`);
    }
    if (game.update_scheduled != null) {
      const s = document.createElement("span");
      s.textContent = `Scheduled ${formatDateTime(game.update_scheduled)}`;
      s.title = "When the launcher plans to start this update";
      parts.push(s);
    }
    if (game.status === "update_available" && !result.can_update) {
      parts.push("Start this update from the launcher itself");
    }
  }
  if (game.installed_build) parts.push(`Build ${game.installed_build}`);
  if (game.last_updated != null) parts.push(`Updated ${formatDate(game.last_updated)}`);
  if (game.last_played != null) parts.push(`Played ${formatDate(game.last_played)}`);
  if (game.install_date) parts.push(`Installed ${formatInstallDate(game.install_date)}`);

  if (parts.length === 0) return null;
  const line = document.createElement("div");
  line.className = "row-sub";
  parts.forEach((p, i) => {
    if (i > 0) line.append(" · ");
    line.append(p);
  });
  return line;
}

function buildGameRow(game: Game, result: ProviderResult): HTMLElement {
  const row = document.createElement("div");
  row.className = "game-row";
  row.setAttribute("role", "listitem");
  row.dataset.key = gameKey(game);
  row.dataset.name = `${game.name} ${launcherLabel(game.launcher, result.name)}`.toLowerCase();
  row.dataset.status = game.status;

  const status = document.createElement("span");
  status.className = `status status-${game.status}`;
  status.textContent = STATUS_LABEL[game.status];

  const nameWrap = document.createElement("span");
  nameWrap.className = "game-name";
  const name = document.createElement("span");
  name.className = "game-name-text";
  name.textContent = game.name;
  name.title = game.name;
  nameWrap.append(buildGameIcon(game), name);
  if (game.size_bytes != null) {
    const size = document.createElement("span");
    size.className = "game-size";
    const approx = APPROX_SIZE_LAUNCHERS.has(game.launcher);
    size.textContent = `${approx ? "approx. " : ""}${formatSize(game.size_bytes)}`;
    if (approx) size.title = "Approximate: the installer's own size estimate";
    nameWrap.append(size);
  }

  const actions = document.createElement("span");
  actions.className = "row-actions";

  // Only "update_available" gets the amber Update button — amber means
  // actionable, and re-triggering an update that is already running is not.
  if (game.status === "update_available" && result.can_update) {
    const btn = document.createElement("button");
    btn.className = "btn-update";
    btn.textContent = "Update";
    btn.dataset.focusId = `update:${row.dataset.key}`;
    btn.setAttribute("aria-label", `Update ${game.name}`);
    btn.onclick = () => {
      btn.disabled = true;
      invoke("trigger_update", { launcher: game.launcher, gameId: game.id })
        .then(() => showRowMsg(row, `Update request sent to ${launcherLabel(game.launcher, result.name)}.`, false))
        .catch((err) => showRowMsg(row, `Update failed: ${err}`, true))
        .finally(() => {
          btn.disabled = false;
        });
    };
    actions.append(btn);
  }

  if (game.status === "update_available") {
    const ign = document.createElement("button");
    ign.className = "btn-quiet";
    ign.textContent = "Ignore";
    ign.dataset.focusId = `ignore:${row.dataset.key}`;
    ign.setAttribute("aria-label", `Ignore update for ${game.name}`);
    ign.title = "Leave this game out of the counts and Update all (restore in Settings)";
    ign.onclick = () => {
      ignored.set(gameKey(game), `${game.name} (${launcherLabel(game.launcher, result.name)})`);
      saveIgnored();
      setAction(`Ignoring "${game.name}". Restore it in Settings.`, 8000);
      rebuildAll();
    };
    actions.append(ign);
  }

  if (result.can_launch) {
    const launchBtn = document.createElement("button");
    launchBtn.className = "btn-launch";
    launchBtn.textContent = "Launch";
    launchBtn.dataset.focusId = `launch:${row.dataset.key}`;
    launchBtn.setAttribute("aria-label", `Launch ${game.name}`);
    launchBtn.onclick = () => {
      launchBtn.disabled = true;
      invoke("launch_game", { launcher: game.launcher, gameId: game.id })
        .catch((err) => showRowMsg(row, `Launch failed: ${err}`, true))
        .finally(() => {
          launchBtn.disabled = false;
        });
    };
    actions.append(launchBtn);
  }

  row.append(status, nameWrap, actions);
  const sub = buildSubLine(game, result);
  if (sub) row.append(sub);
  return row;
}

// ------------------------------------------------------------------ lanes
function buildChevron(): HTMLElement {
  const c = document.createElement("span");
  c.className = "chevron";
  c.setAttribute("aria-hidden", "true");
  return c;
}

function buildLauncherSection(launcher: string, result: ProviderResult): HTMLElement {
  const visible = visibleGamesOf(result);
  const ignoredCount = result.games.length - visible.length;
  const needsUpdate = visible.filter(isPending).sort(compareGames);
  const rest = visible.filter((g) => !isPending(g)).sort(compareGames);
  const counts = countPending(visible);
  const pendingTotal = counts.available + counts.updating;
  const label = launcherLabel(launcher, result.name);

  const section = document.createElement("details");
  section.className = "launcher-section";
  section.dataset.launcher = launcher;
  section.dataset.pending = String(pendingTotal);
  section.open = openState.get(launcher) ?? pendingTotal > 0;

  const summary = document.createElement("summary");
  summary.dataset.focusId = `lane:${launcher}`;
  summary.append(buildChevron(), buildLauncherIcon(launcher), buildRunningDot(result.running));
  // The click fires before the browser toggles `open`, so the new value is the opposite.
  summary.addEventListener("click", () => openState.set(launcher, !section.open));

  const title = document.createElement("span");
  title.className = "lane-title";
  title.textContent = label;
  summary.appendChild(title);

  if (pendingTotal > 0) {
    const badge = document.createElement("span");
    badge.className = "lane-badge";
    badge.textContent = pendingLabel(counts);
    summary.appendChild(badge);
  }

  const sized = visible.filter((g) => g.size_bytes != null);
  const totalBytes = sized.reduce((sum, g) => sum + (g.size_bytes ?? 0), 0);
  const meta = document.createElement("span");
  meta.className = "lane-meta";
  let metaText = plural(visible.length, "game");
  if (sized.length > 0) {
    metaText += ` · ${formatSize(totalBytes)}`;
    if (sized.length < visible.length) {
      metaText += ` (${sized.length} of ${visible.length} sized)`;
      meta.title = "Only games with a known size are summed";
    }
  }
  if (ignoredCount > 0) metaText += ` · ${ignoredCount} ignored`;
  meta.textContent = metaText;
  summary.appendChild(meta);
  section.appendChild(summary);

  const body = document.createElement("div");
  body.className = "launcher-body";
  body.setAttribute("role", "list");
  body.setAttribute("aria-label", `${label} games`);

  needsUpdate.forEach((g) => body.appendChild(buildGameRow(g, result)));

  if (rest.length > 0) {
    const restSection = document.createElement("details");
    restSection.className = "rest-section";
    restSection.dataset.defaultOpen = needsUpdate.length === 0 ? "1" : "0";
    restSection.dataset.launcher = launcher;
    restSection.open = restOpenState.get(launcher) ?? needsUpdate.length === 0;
    const restSummary = document.createElement("summary");
    restSummary.dataset.focusId = `rest:${launcher}`;
    restSummary.addEventListener("click", () => restOpenState.set(launcher, !restSection.open));
    const upToDate = rest.filter((g) => g.status === "up_to_date").length;
    const unknown = rest.length - upToDate;
    const bits: string[] = [];
    if (upToDate > 0) bits.push(`${upToDate} up to date`);
    if (unknown > 0) bits.push(`${unknown} unknown`);
    restSummary.append(buildChevron(), bits.join(" · "));
    restSection.appendChild(restSummary);
    const restBody = document.createElement("div");
    restBody.setAttribute("role", "list");
    rest.forEach((g) => restBody.appendChild(buildGameRow(g, result)));
    restSection.appendChild(restBody);
    body.appendChild(restSection);
  }

  section.appendChild(body);
  return section;
}

function buildLoadingPlaceholder(launcher: string): HTMLElement {
  const el = document.createElement("div");
  el.className = "launcher-loading muted";
  el.dataset.launcher = launcher;
  el.textContent = `${launcherLabel(launcher)} — loading…`;
  return el;
}

function buildErrorRow(launcher: string, state: Extract<LaneState, { kind: "error" }>): HTMLElement {
  const el = document.createElement("div");
  el.className = "launcher-error";
  el.dataset.launcher = launcher;
  el.setAttribute("role", "alert");
  const glyph = document.createElement("span");
  glyph.className = "state-glyph";
  glyph.setAttribute("aria-hidden", "true");
  glyph.textContent = "!";
  const text = document.createElement("span");
  text.className = "error-text";
  text.textContent = `${launcherLabel(launcher, state.name)} — check failed: ${state.message}`;
  const retry = document.createElement("button");
  retry.textContent = state.retrying ? "Retrying…" : "Retry";
  retry.disabled = state.retrying;
  retry.dataset.focusId = `retry:${launcher}`;
  retry.setAttribute("aria-label", `Retry ${launcherLabel(launcher, state.name)}`);
  retry.onclick = () => void retryLane(launcher);
  el.append(glyph, text, retry);
  return el;
}

function buildConnectedRow(result: ProviderResult): HTMLElement {
  const row = document.createElement("div");
  row.className = "connected-row";
  row.setAttribute("role", "listitem");
  row.dataset.launcher = result.id;
  const name = document.createElement("span");
  name.append(buildLauncherIcon(result.id), buildRunningDot(result.running), launcherLabel(result.id, result.name));
  const btn = document.createElement("button");
  btn.textContent = "Open";
  btn.dataset.focusId = `open:${result.id}`;
  btn.setAttribute("aria-label", `Open ${launcherLabel(result.id, result.name)}`);
  btn.onclick = () => {
    btn.disabled = true;
    invoke("trigger_update", { launcher: result.id, gameId: "" })
      .catch((err) => showRowMsg(row, `Open failed: ${err}`, true))
      .finally(() => {
        btn.disabled = false;
      });
  };
  row.append(name, btn);
  return row;
}

// Launchers that are installed but expose no game list.
function renderConnected() {
  const list = $("#game-list");
  if (!list) return;
  list.querySelector(".connected-only")?.remove();
  const results: ProviderResult[] = [];
  for (const id of launcherIds) {
    const s = lanes.get(id);
    if (s?.kind === "ok" && s.result.games.length === 0) results.push(s.result);
  }
  if (results.length === 0) return;
  const section = document.createElement("div");
  section.className = "connected-only";
  const heading = document.createElement("p");
  heading.className = "muted";
  heading.textContent = "Connected (no game list available):";
  const wrap = document.createElement("div");
  wrap.setAttribute("role", "list");
  results.forEach((r) => wrap.appendChild(buildConnectedRow(r)));
  section.append(heading, wrap);
  list.appendChild(section);
}

function laneElement(id: string, state: LaneState): HTMLElement | null {
  switch (state.kind) {
    case "loading":
      return buildLoadingPlaceholder(id);
    case "error":
      return buildErrorRow(id, state);
    case "ok":
      return state.result.games.length > 0 ? buildLauncherSection(id, state.result) : null;
    default:
      return null;
  }
}

// Urgency order: lanes with updates, then failed checks, then the rest; ties keep backend order.
function sortedLaneIds(): string[] {
  const rank = (id: string): number => {
    const s = lanes.get(id);
    if (s?.kind === "ok" && visibleGamesOf(s.result).some(isPending)) return 0;
    if (s?.kind === "error") return 1;
    return 2;
  };
  return [...launcherIds].sort((a, b) => rank(a) - rank(b) || launcherIds.indexOf(a) - launcherIds.indexOf(b));
}

function reorderList() {
  const list = $("#game-list");
  if (!list) return;
  // Moving a node drops focus; don't shuffle under a keyboard user.
  if (document.activeElement && list.contains(document.activeElement)) return;
  const want = sortedLaneIds()
    .map((id) => laneEls.get(id))
    .filter((el): el is HTMLElement => !!el);
  want.forEach((el, i) => {
    if (list.children[i] !== el) list.insertBefore(el, list.children[i] ?? null);
  });
  const connected = list.querySelector(".connected-only");
  if (connected && list.lastElementChild !== connected) list.appendChild(connected);
}

// Replaces one lane's DOM in place (no full wipe), keeping everything else untouched.
function renderLane(id: string) {
  const list = $("#game-list");
  if (!list) return;
  list.querySelector("#initial-loading")?.remove();
  const state = lanes.get(id);
  const old = laneEls.get(id);
  const next = state ? laneElement(id, state) : null;
  if (next) {
    if (old && old.isConnected) old.replaceWith(next);
    else list.appendChild(next);
    laneEls.set(id, next);
  } else {
    old?.remove();
    laneEls.delete(id);
  }
}

function setLane(id: string, state: LaneState) {
  withFocusKept(() => {
    lanes.set(id, state);
    renderLane(id);
    renderConnected();
  });
}

function rebuildAll() {
  const container = $("#container");
  const top = container?.scrollTop ?? 0;
  withFocusKept(() => {
    for (const id of launcherIds) renderLane(id);
    renderConnected();
    reorderList();
  });
  if (container) container.scrollTop = top;
  updateStats();
  applyFilters();
  renderNotDetected();
}

// ------------------------------------------------------------------ stats
function updateStats() {
  const statsEl = $("#stats");
  const updateAllBtn = $<HTMLButtonElement>("#update-all-btn");
  if (!statsEl) return;

  const visible = allVisibleGames();
  const ignoredCount = allGames().length - visible.length;
  const counts = countPending(visible);
  const label = pendingLabel(counts);
  statsEl.textContent = `${plural(visible.length, "game")} · ${label || "no updates"}${ignoredCount > 0 ? ` · ${ignoredCount} ignored` : ""}`;
  statsEl.title = statsEl.textContent;
  statsEl.classList.toggle("stats-pending", counts.available > 0);

  if (updateAllBtn && !updateAllRunning) {
    const n = updatableGames().length;
    updateAllBtn.hidden = n === 0;
    updateAllBtn.textContent = `Update all (${n})`;
    updateAllBtn.setAttribute("aria-label", `Update all, ${plural(n, "game")}`);
  }
}

function setLastChecked(text: string) {
  const el = $("#last-checked");
  if (el) el.textContent = text;
}

function renderNotDetected() {
  const det = $("#not-detected");
  const summary = $("#not-detected-summary");
  const list = $("#not-detected-list");
  if (!det || !summary || !list) return;
  const names = launcherIds.filter((id) => lanes.get(id)?.kind === "absent").map((id) => launcherLabel(id));
  det.hidden = loadingNow || names.length === 0;
  summary.textContent = `Not found on this PC (${names.length})`;
  list.textContent = `${names.join(", ")}. LaunchPilot looked for these launchers and found no install.`;
}

// ---------------------------------------------------------------- loading
async function fetchLauncher(id: string): Promise<LaneState> {
  try {
    const result = await invoke<ProviderResult | null>("provider_data", { launcher: id });
    if (!result) return { kind: "absent" };
    // A failed read is not "no games": say so.
    if (result.error) return { kind: "error", name: result.name, message: result.error, retrying: false };
    return { kind: "ok", result };
  } catch (err) {
    return { kind: "error", name: launcherLabel(id), message: String(err), retrying: false };
  }
}

async function retryLane(id: string) {
  const cur = lanes.get(id);
  if (cur?.kind === "error") setLane(id, { ...cur, retrying: true });
  const next = await fetchLauncher(id);
  setLane(id, next);
  finishPass();
}

// Loads each launcher independently and renders it the moment it resolves.
// Existing lanes stay on screen until their replacement arrives (no wipe).
async function loadGames() {
  const list = $("#game-list");
  if (!list) return;

  if (updateAllRunning) {
    setAction("Refresh queued until Update all finishes.");
    return;
  }
  const requestId = ++loadRequestId;

  let ids: string[];
  try {
    ids = await invoke<string[]>("launcher_ids");
  } catch (err) {
    if (requestId !== loadRequestId) return;
    loadingNow = false;
    list.setAttribute("aria-busy", "false");
    list.querySelector("#initial-loading")?.remove();
    const empty = $("#empty-state");
    if (empty) {
      empty.hidden = false;
      empty.textContent = `Failed to list launchers: ${err}`;
    }
    return;
  }
  if (requestId !== loadRequestId) return;

  launcherIds = ids;
  loadingNow = true;
  list.setAttribute("aria-busy", "true");
  $("#refresh-btn")?.setAttribute("aria-busy", "true");
  announce("Refreshing launchers…");
  for (const id of ids) {
    if (!lanes.has(id)) setLane(id, { kind: "loading" });
  }
  updateStats();
  renderNotDetected();

  let settled = 0;
  setLastChecked(`Checking launchers 0/${ids.length}…`);
  ids.forEach((id) => {
    void fetchLauncher(id).then((state) => {
      if (requestId !== loadRequestId) return;
      settled += 1;
      setLane(id, state);
      updateStats();
      applyFilters();
      if (settled < ids.length) {
        setLastChecked(`Checking launchers ${settled}/${ids.length}…`);
        return;
      }
      loadingNow = false;
      list.setAttribute("aria-busy", "false");
      $("#refresh-btn")?.setAttribute("aria-busy", "false");
      finishPass();
      setLastChecked(`Last checked ${new Date().toLocaleTimeString()}`);
      const c = countPending(allVisibleGames());
      announce(`Refresh finished. ${plural(allVisibleGames().length, "game")}, ${pendingLabel(c) || "no updates"}.`);
    });
  });
}

// After all lanes of a pass (or one retry) have settled.
function finishPass() {
  reorderList();
  renderNotDetected();
  updateStats();
  applyFilters();
  void announcePending();
}

// ------------------------------------------------- tray count + notifications
interface ShellSettings {
  tray: boolean;
  notify: boolean;
  autostart: boolean;
}
let shellSettings: ShellSettings = { tray: false, notify: false, autostart: false };

// launcher id -> keys (`launcher:gameId`) of its games that were update_available
// or updating the last time that launcher was read successfully. A launcher
// absent from the map has never been read OK in this session: its first
// successful read only seeds the map, so starting the app never notifies.
const knownPending = new Map<string, Set<string>>();

// Real transitions only: update_available now, and not pending at the last
// good read of that launcher. A failed/absent lane keeps its previous state
// (so a flaky launcher can't re-notify), "updating" counts as known (so
// updating -> update_available doesn't re-notify), Unknown never qualifies.
function takeNewUpdates(): string[] {
  const fresh: string[] = [];
  for (const [id, lane] of lanes) {
    if (lane.kind !== "ok") continue;
    const now = new Set<string>();
    for (const g of lane.result.games) if (isPending(g)) now.add(gameKey(g));
    const prev = knownPending.get(id);
    if (prev) {
      for (const g of lane.result.games) {
        const k = gameKey(g);
        if (g.status === "update_available" && !prev.has(k) && !ignored.has(k)) fresh.push(g.name);
      }
    }
    knownPending.set(id, now);
  }
  return fresh;
}

async function announcePending() {
  // Always run the diff, so the baseline stays current even with notifications off.
  const fresh = takeNewUpdates();
  try {
    await invoke("set_pending", { count: countPending(allVisibleGames()).available });
    // A toast over the window you are looking at is noise: only when it is hidden/unfocused.
    if (shellSettings.notify && fresh.length > 0 && !document.hasFocus()) {
      await invoke("notify_updates", { names: fresh });
    }
  } catch {
    /* tray/toast are best-effort; the list in the window is the source of truth */
  }
}

// -------------------------------------------------------------- filtering
function applyFilters() {
  const search = $<HTMLInputElement>("#search");
  const query = search?.value.trim().toLowerCase() ?? "";
  const filtering = query !== "" || statusFilter !== "all";
  let anyMatch = false;

  const matches = (row: HTMLElement): boolean => {
    if (query !== "" && !(row.dataset.name ?? "").includes(query)) return false;
    const st = row.dataset.status;
    if (statusFilter === "all") return true;
    if (statusFilter === "pending") return st === "update_available" || st === "updating";
    return st === statusFilter;
  };

  document.querySelectorAll<HTMLDetailsElement>(".launcher-section").forEach((section) => {
    let sectionHasMatch = false;
    section.querySelectorAll<HTMLElement>(":scope > .launcher-body > .game-row").forEach((row) => {
      const m = matches(row);
      row.hidden = !m;
      if (m) sectionHasMatch = true;
    });
    section.querySelectorAll<HTMLDetailsElement>(".rest-section").forEach((rest) => {
      let restHasMatch = false;
      rest.querySelectorAll<HTMLElement>(".game-row").forEach((row) => {
        const m = matches(row);
        row.hidden = !m;
        if (m) restHasMatch = true;
      });
      if (restHasMatch) sectionHasMatch = true;
      rest.hidden = filtering && !restHasMatch;
      // While filtering, show the matches; when cleared, go back to what the user chose.
      rest.open = filtering ? restHasMatch : (restOpenState.get(rest.dataset.launcher ?? "") ?? rest.dataset.defaultOpen === "1");
    });
    section.hidden = filtering && !sectionHasMatch;
    section.open = filtering ? sectionHasMatch : (openState.get(section.dataset.launcher ?? "") ?? section.dataset.pending !== "0");
    if (sectionHasMatch) anyMatch = true;
  });

  const hasLanes = document.querySelector(".launcher-section") !== null;
  const empty = $("#empty-state");
  if (!empty) return;
  let text = "";
  if (filtering && hasLanes && !anyMatch) {
    text = query !== "" ? `No games match "${search?.value.trim()}".` : "No games with this status.";
  } else if (!loadingNow && launcherIds.length > 0 && ![...lanes.values()].some((s) => s.kind === "ok" || s.kind === "error")) {
    text = "No games found (no supported launcher detected, or none installed).";
  }
  empty.textContent = text;
  empty.hidden = text === "";
}

// ------------------------------------------------------------- Update all
function updatableGames(): Game[] {
  const out: Game[] = [];
  for (const state of lanes.values()) {
    if (state.kind !== "ok" || !state.result.can_update) continue;
    out.push(...visibleGamesOf(state.result).filter((g) => g.status === "update_available"));
  }
  return out;
}

function openUpdateAllConfirm() {
  if (updateAllRunning) return;
  const pending = updatableGames();
  const dlg = $<HTMLDialogElement>("#confirm-panel");
  if (pending.length === 0 || !dlg) return;

  const byLauncher = new Map<string, number>();
  pending.forEach((g) => byLauncher.set(g.launcher, (byLauncher.get(g.launcher) ?? 0) + 1));
  const list = $("#confirm-list");
  if (list) {
    list.replaceChildren();
    for (const [launcher, n] of byLauncher) {
      const li = document.createElement("li");
      li.textContent = `${launcherLabel(launcher)}: ${plural(n, "game")}`;
      list.append(li);
    }
  }
  const known = pending.filter((g) => g.update_bytes_total != null);
  const total = known.reduce((s, g) => s + (g.update_bytes_total ?? 0), 0);
  const unknown = pending.length - known.length;
  const body = $("#confirm-body");
  if (body) body.textContent = `Send update requests for ${plural(pending.length, "game")}?`;
  const sizeEl = $("#confirm-size");
  if (sizeEl) {
    if (known.length === 0) sizeEl.textContent = "Download size unknown for all of these games.";
    else
      sizeEl.textContent =
        `Known download size: ${formatSize(total)} (${known.length} of ${pending.length} games)` +
        (unknown > 0 ? `; size unknown for ${plural(unknown, "game")}.` : ".");
  }
  dlg.returnValue = "";
  dlg.showModal();
  void fillConfirmSpace(pending);
}

interface DriveSpace {
  drive: string;
  required_bytes: number;
  unknown_size: number;
  games: number;
  free_bytes: number | null;
}

let confirmSpaceToken = 0;

// Free space on the drive(s) the pending downloads land on. Shown after the
// dialog opens (the check runs on a worker thread); a warning never blocks.
async function fillConfirmSpace(pending: Game[]) {
  const token = ++confirmSpaceToken;
  const list = $("#confirm-space");
  const warn = $("#confirm-warn");
  list?.replaceChildren();
  if (warn) warn.hidden = true;
  let report: { drives: DriveSpace[]; unplaced: number };
  try {
    report = await invoke("update_space", {
      items: pending.map((g) => ({ launcher: g.launcher, game_id: g.id, bytes: g.update_bytes_total })),
    });
  } catch {
    return; // the size line above is still accurate; free space just isn't shown
  }
  if (token !== confirmSpaceToken || !list) return;
  const short: string[] = [];
  for (const d of report.drives) {
    const li = document.createElement("li");
    const need = d.required_bytes > 0 ? `${formatSize(d.required_bytes)} to download` : "download size unknown";
    const extra = d.unknown_size > 0 && d.required_bytes > 0 ? ` (+ ${plural(d.unknown_size, "game")} of unknown size)` : "";
    const free = d.free_bytes != null ? `${formatSize(d.free_bytes)} free on ${d.drive}` : `free space on ${d.drive} unreadable`;
    li.textContent = `${d.drive} ${need}${extra} · ${free}`;
    list.append(li);
    if (d.free_bytes != null && d.free_bytes < d.required_bytes) {
      short.push(`${d.drive} needs at least ${formatSize(d.required_bytes)} but has ${formatSize(d.free_bytes)} free`);
    }
  }
  if (report.unplaced > 0) {
    const li = document.createElement("li");
    li.textContent = `Drive unknown for ${plural(report.unplaced, "game")} (only Steam downloads are placed on a drive).`;
    list.append(li);
  }
  if (warn && short.length > 0) {
    warn.textContent = `Not enough free space: ${short.join("; ")}. You can still continue, but the download may stall.`;
    warn.hidden = false;
  }
}

async function runUpdateAll() {
  const btn = $<HTMLButtonElement>("#update-all-btn");
  const pending = updatableGames();
  if (pending.length === 0 || !btn || updateAllRunning) return;

  updateAllRunning = true;
  btn.disabled = true;
  btn.setAttribute("aria-busy", "true");
  const failed: Array<{ game: Game; err: unknown }> = [];
  for (let i = 0; i < pending.length; i++) {
    const game = pending[i];
    btn.textContent = `Sending ${i + 1}/${pending.length}…`;
    try {
      await invoke("trigger_update", { launcher: game.launcher, gameId: game.id });
    } catch (err) {
      failed.push({ game, err });
      const row = document.querySelector<HTMLElement>(`.game-row[data-key="${CSS.escape(gameKey(game))}"]`);
      if (row) showRowMsg(row, `Update failed: ${err}`, true);
    }
    await new Promise((r) => setTimeout(r, 400));
  }
  const sent = pending.length - failed.length;
  setAction(
    `Update requests sent for ${sent} of ${plural(pending.length, "game")}` +
      (failed.length > 0 ? `; ${failed.length} failed (see the rows)` : "") +
      ". Statuses below come from the launchers.",
    20000,
  );
  announce($("#action-status")?.textContent ?? "");
  // Give the launchers a moment to write their new state, then re-read it.
  await new Promise((r) => setTimeout(r, 1500));
  updateAllRunning = false;
  btn.disabled = false;
  btn.setAttribute("aria-busy", "false");
  await loadGames();
}

// -------------------------------------------------------------- refresh timer
function getRefreshMinutes(): number {
  const stored = store.get(REFRESH_INTERVAL_KEY);
  const n = stored ? parseInt(stored, 10) : DEFAULT_REFRESH_MINUTES;
  return Number.isFinite(n) ? n : DEFAULT_REFRESH_MINUTES;
}

function applyRefreshInterval() {
  if (refreshTimer) window.clearInterval(refreshTimer);
  const minutes = getRefreshMinutes();
  if (minutes <= 0) return;
  refreshTimer = window.setInterval(() => {
    if (updateAllRunning || loadingNow) return; // never race Update all or a running refresh
    void loadGames();
    void checkForUpdates();
  }, minutes * 60 * 1000);
}

// ------------------------------------------------------------- self-update
let updateInFlight = false; // checking or installing: one at a time
let pendingUpdate: Update | null = null;

function setUpdateBanner(text: string, showInstall: boolean, installDisabled = false) {
  const banner = $("#update-banner");
  const statusEl = $("#update-status");
  const installBtn = $<HTMLButtonElement>("#update-install-btn");
  if (!banner || !statusEl || !installBtn) return;
  statusEl.textContent = text;
  installBtn.hidden = !showInstall;
  installBtn.disabled = installDisabled;
  banner.hidden = text === "";
}

async function checkForUpdates() {
  // Never while another check/install runs, never during Update all, and
  // once an update is known there is nothing more to check.
  if (updateInFlight || updateAllRunning || pendingUpdate) return;
  updateInFlight = true;
  try {
    const update = await check();
    if (!update) {
      setUpdateBanner("", false);
      return;
    }
    pendingUpdate = update;
    setUpdateBanner(`Update available: v${update.version}.`, true);
  } catch (err) {
    // No retry/backoff: the next scheduled check (or launch) tries again.
    setUpdateBanner(`Update check failed: ${err}`, false);
  } finally {
    updateInFlight = false;
  }
}

// Only ever runs from the user's click on "Install & restart".
async function installUpdate() {
  if (!pendingUpdate || updateInFlight) return;
  if (updateAllRunning) {
    setUpdateBanner(`Update available: v${pendingUpdate.version}. Wait for Update all to finish, then install.`, true);
    return;
  }
  updateInFlight = true;
  const version = pendingUpdate.version;
  setUpdateBanner(`Downloading v${version}…`, true, true);
  let total: number | undefined;
  let received = 0;
  try {
    await pendingUpdate.downloadAndInstall((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength;
      else if (ev.event === "Progress") {
        received += ev.data.chunkLength;
        const pct = total ? ` ${Math.min(100, Math.floor((received / total) * 100))}%` : "";
        setUpdateBanner(`Downloading v${version}…${pct}`, true, true);
      } else if (ev.event === "Finished") setUpdateBanner(`Installing v${version}…`, true, true);
    });
    setUpdateBanner("Update installed. Restarting…", false);
    await relaunch();
  } catch (err) {
    updateInFlight = false;
    setUpdateBanner(`Update to v${version} failed: ${err}`, true);
  }
}

// ----------------------------------------------------------------- report
function buildReport(): string {
  const games = allGames();
  const esc = (s: string) => s.replace(/\|/g, "\\|");
  const dash = "—";
  const lines = [
    "# LaunchPilot library report",
    "",
    `Generated ${new Date().toLocaleString()}. ${plural(games.length, "game")}.`,
    "",
    "| Launcher | Game | Status | Size | Build | Last played | Last updated |",
    "|---|---|---|---|---|---|---|",
  ];
  for (const state of lanes.values()) {
    if (state.kind !== "ok") continue;
    for (const g of [...state.result.games].sort(compareGames)) {
      const size = g.size_bytes == null ? dash : `${APPROX_SIZE_LAUNCHERS.has(g.launcher) ? "approx. " : ""}${formatSize(g.size_bytes)}`;
      lines.push(
        `| ${esc(launcherLabel(g.launcher, state.result.name))} | ${esc(g.name)} | ${STATUS_LABEL[g.status]}${ignored.has(gameKey(g)) ? " (ignored)" : ""} | ${size} | ${esc(g.installed_build ?? dash)} | ${g.last_played != null ? formatDate(g.last_played) : dash} | ${g.last_updated != null ? formatDate(g.last_updated) : dash} |`,
      );
    }
  }
  return lines.join("\n") + "\n";
}

async function copyText(text: string): Promise<void> {
  try {
    // The API can reject (no focus / no permission) or, in some WebViews, never settle: don't wait forever.
    await Promise.race([
      navigator.clipboard.writeText(text),
      new Promise<never>((_, reject) => window.setTimeout(() => reject(new Error("clipboard API timed out")), 2000)),
    ]);
    return;
  } catch {
    /* fall through to the textarea route */
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "");
  ta.className = "visually-hidden";
  document.body.appendChild(ta);
  ta.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } finally {
    ta.remove();
  }
  if (!ok) throw new Error("the clipboard is not available");
}

async function copyReport() {
  if (allGames().length === 0) {
    setAction("Nothing to copy yet.", 5000);
    return;
  }
  try {
    await copyText(buildReport());
    setAction("Library report copied to the clipboard.", 5000);
  } catch (err) {
    setAction(`Could not copy the report: ${err}`, 10000);
  }
}

// -------------------------------------------------------------- settings UI
function renderIgnoredList() {
  const ul = $("#ignored-list");
  const empty = $("#ignored-empty");
  if (!ul || !empty) return;
  ul.replaceChildren();
  empty.hidden = ignored.size > 0;
  for (const [key, label] of ignored) {
    const li = document.createElement("li");
    const text = document.createElement("span");
    text.textContent = label;
    const btn = document.createElement("button");
    btn.textContent = "Restore";
    btn.setAttribute("aria-label", `Restore ${label}`);
    btn.onclick = () => {
      ignored.delete(key);
      saveIgnored();
      renderIgnoredList();
      rebuildAll();
    };
    li.append(text, btn);
    ul.append(li);
  }
}

function setupDialog(dlg: HTMLDialogElement | null) {
  // The dialog has no padding, so a click whose target is the dialog itself hit the backdrop.
  dlg?.addEventListener("click", (e) => {
    if (e.target === dlg) dlg.close();
  });
}

// Opt-in tray / notification / autostart toggles (all off by default; the
// backend persists them and reads the real autostart state from the registry).
function setupShellOptions() {
  const boxes = {
    tray: $<HTMLInputElement>("#opt-tray"),
    notify: $<HTMLInputElement>("#opt-notify"),
    autostart: $<HTMLInputElement>("#opt-autostart"),
  };
  const keys = Object.keys(boxes) as (keyof ShellSettings)[];
  const err = $("#opt-error");
  const show = (s: ShellSettings) => {
    shellSettings = s;
    for (const k of keys) {
      const b = boxes[k];
      if (b) b.checked = s[k];
    }
  };
  const refresh = () =>
    invoke<ShellSettings>("get_settings")
      .then(show)
      .catch(() => {});
  void refresh();
  $("#settings-btn")?.addEventListener("click", () => void refresh());
  for (const k of keys) {
    const box = boxes[k];
    box?.addEventListener("change", () => {
      const wanted = box.checked;
      if (err) err.hidden = true;
      invoke<ShellSettings>("set_setting", { key: k, value: wanted })
        .then(show)
        .catch((e) => {
          box.checked = !wanted; // the change did not happen: show the truth
          if (err) {
            err.textContent = `Could not change this setting: ${e}`;
            err.hidden = false;
          }
        });
    });
  }
}

function setupControls() {
  const search = $<HTMLInputElement>("#search");
  search?.addEventListener("input", applyFilters);
  $("#refresh-btn")?.addEventListener("click", () => void loadGames());
  $("#update-all-btn")?.addEventListener("click", openUpdateAllConfirm);
  $("#copy-report-btn")?.addEventListener("click", () => void copyReport());
  $("#update-install-btn")?.addEventListener("click", () => void installUpdate());

  const chips = document.querySelectorAll<HTMLButtonElement>(".chip");
  chips.forEach((chip) =>
    chip.addEventListener("click", () => {
      statusFilter = (chip.dataset.filter as StatusFilter) ?? "all";
      chips.forEach((c) => c.setAttribute("aria-pressed", String(c === chip)));
      applyFilters();
    }),
  );

  const sortSelect = $<HTMLSelectElement>("#sort-select");
  if (sortSelect) {
    const saved = store.get(SORT_KEY);
    if (saved === "name" || saved === "status" || saved === "last_played" || saved === "size") sortMode = saved;
    sortSelect.value = sortMode;
    sortSelect.addEventListener("change", () => {
      sortMode = sortSelect.value as SortMode;
      store.set(SORT_KEY, sortMode);
      rebuildAll();
    });
  }

  const settings = $<HTMLDialogElement>("#settings-panel");
  setupDialog(settings);
  $("#settings-btn")?.addEventListener("click", () => {
    renderIgnoredList();
    settings?.showModal();
  });

  const confirm = $<HTMLDialogElement>("#confirm-panel");
  setupDialog(confirm);
  confirm?.addEventListener("close", () => {
    if (confirm.returnValue === "go") void runUpdateAll();
  });

  const intervalSelect = $<HTMLSelectElement>("#interval-select");
  if (intervalSelect) {
    intervalSelect.value = String(getRefreshMinutes());
    intervalSelect.addEventListener("change", () => {
      store.set(REFRESH_INTERVAL_KEY, intervalSelect.value);
      applyRefreshInterval();
    });
  }

  setupShellOptions();

  const themeSelect = $<HTMLSelectElement>("#theme-select");
  if (themeSelect) {
    themeSelect.value = getTheme();
    themeSelect.addEventListener("change", () => {
      const t = themeSelect.value as Theme;
      store.set(THEME_KEY, t);
      applyTheme(t);
    });
  }

  document.addEventListener("keydown", (e) => {
    const modalOpen = document.querySelector("dialog[open]") !== null;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (mod && key === "f") {
      e.preventDefault();
      if (!modalOpen && search) {
        search.focus();
        search.select();
      }
    } else if (e.key === "F5" || (mod && key === "r")) {
      e.preventDefault(); // otherwise WebView2 reloads the whole page
      if (!modalOpen) void loadGames();
    } else if (e.key === "Escape" && !modalOpen && search && search.value !== "") {
      search.value = "";
      applyFilters();
    }
  });
}

function setupOnboarding() {
  const dlg = $<HTMLDialogElement>("#onboarding-panel");
  if (!dlg) return;
  setupDialog(dlg);
  dlg.addEventListener("close", () => store.set(ONBOARDING_SEEN_KEY, "1"));
  if (!store.get(ONBOARDING_SEEN_KEY)) dlg.showModal();
}

window.addEventListener("DOMContentLoaded", () => {
  loadIgnored();
  setupControls();
  setupOnboarding();
  void checkForUpdates();
  void loadGames();
  applyRefreshInterval();
  // Tray menu "Refresh now" (the webview keeps running while hidden to tray).
  void listen("lp-refresh", () => {
    if (!updateAllRunning && !loadingNow) void loadGames();
  });
});
