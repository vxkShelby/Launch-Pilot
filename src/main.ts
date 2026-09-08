import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

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
});
