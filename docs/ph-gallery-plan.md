# LaunchPilot: Product Hunt gallery plan

Spec source: Product Hunt's product-page guide (recommended gallery size 1270x760 px; at least 1 image, 3 or more
recommended; logo 240x240, JPG/PNG/GIF, up to 2 MB). Verify against the live guide before upload.

Rules for every shot:
- Real app, real library, real launchers. No mock data, no retouched status text. The project rule is: never claim
  what the app doesn't do.
- Capture after the UI work lands, from the release build (not `tauri dev`), so the version in the shot is the shipped one.
- Canvas is 1270x760, background `#121316` (`--bg-0`). The app window (default 1100x720, 1.53:1) is inset and centred,
  never stretched. A 1px `#2a2b30` border and the existing soft shadow are fine; no device frames, no gradients.
- Windows scale 100% for capture if possible. Export PNG. Keep each under 2 MB.
- Before every capture: hide the taskbar clutter, close unrelated windows, and make sure no game names are
  personal or sensitive. Crop or choose another library state if they are.
- Captions are overlaid text in the page palette (`--ink` on the canvas, 28-32 px, max ~12 words). Do not add claims
  beyond the caption text given here.

## 1. Hero: what needs updating, across everything

- State: at least two launchers with a real "Update available" game (amber Update button visible) and the header's
  "Update all (N)" button showing. Other launchers collapsed or showing Up to date. Auto-refresh set to the default
  (30 minutes). Search box empty.
- If your real library has no pending updates, wait for one rather than forcing a fake state.
- Steps: launch the app, let every launcher finish loading (no "loading..." rows), dismiss the onboarding dialog
  if it appears, expand the sections that hold the update rows, capture the window.
- Caption: "Every launcher's pending updates, in one place."

## 2. Unknown instead of guessing

- State: close crop (about 900x500 of the window, scaled up to fill the canvas) of one or two launcher sections showing
  a mix of Up to date and Unknown rows. Ubisoft Connect and Riot Client are the launchers that always report Unknown
  by design, so use one of them next to Steam or GOG.
- Steps: expand those sections, scroll so both are in view, capture and crop. Do not crop out the status labels.
- Caption: "If a launcher doesn't expose update info, we say Unknown. We don't guess."

## 3. Before / after

- State, left half: the real launcher windows you'd otherwise open, tiled on a real desktop (Steam, GOG Galaxy, EA app,
  Battle.net or whichever are installed). Right half: the single LaunchPilot window.
- Steps: arrange launcher windows on the left ~55% of a full-desktop screenshot, LaunchPilot alone on the right, capture
  both at the same scale, compose onto the 1270x760 canvas with a thin divider. Only show launchers you actually have
  installed.
- Caption: "Four launcher windows, or one."  (Adjust the number to the number of windows actually shown.)

## 4. Per-launcher truth table

- State: designed graphic, not a screenshot. Reproduce the README matrix ("What's real, per launcher") in the app
  palette: Detect / Game list / Update status / Launch button columns, one row per launcher. Use the exact
  wording from README.md; if the matrix changes, regenerate this from it.
- Status wording must stay as in README: Steam, GOG, Wargaming, Prism = update status; Epic, EA, Battle.net = live
  "Updating"; Ubisoft, Riot, Age of the Ring = Unknown.
- Caption: "What it can see per launcher. The gaps are real, and we show them."

## 5. Privacy frame

- State: the first-run welcome dialog as shipped. It only shows once per profile (localStorage key `lp.onboardingSeen`).
- Steps: from a clean state, clear that key (or use a fresh Windows user / WebView profile), start the release build,
  capture while the dialog is up over the dashboard.
- Caption: "No accounts. No telemetry. Everything is read locally."
- Do not add an installer size to this frame unless re-measured on the shipping release (the v0.8.3 installer was
  2,445,734 bytes, about 2.3 MB; re-check for the release you ship).

## 6. Short loop (GIF or video, 10-15 s)

- Recommended size 1270x760 if a video is uploaded; keep the GIF under Product Hunt's upload limit (check the guide).
- State: library with at least one real pending update.
- Steps: (1) start the app and show launcher sections loading in one by one; (2) all sections settle; (3) click Update
  on one game; (4) the launcher (e.g. Steam) opens on that game; (5) stop. If the end state isn't what the app
  actually does at that moment, trim the clip instead of staging it.
- Caption (on the first frame / description): "Launch, see what needs updating, click Update."

## 7. Light theme (conditional)

- Only produce this if a light theme ships in the release being launched. If it doesn't, skip it and use the Launch-button
  shot below instead.
- Fallback shot 7: one launcher section (Steam or GOG) showing the Launch button next to game rows. Caption:
  "Launch games straight from the list (Steam, GOG, EA, Ubisoft)."
- Do not show a Launch button for launchers that don't have one (Epic, Battle.net, Riot, Prism, Wargaming, Age of the Ring).

## Also needed

- Logo: `docs/logo.png` resized to 240x240 (confirm it is square and under 2 MB).
- First-comment / tagline text must match README claims. Do not state numbers of users, downloads or supported games
  unless read from the live release page at the time of posting.
- Replace `docs/screenshot.png` (used by the landing page and README) with shot 1 once captured; the current file is
  older than v0.8.3. If the new image dimensions differ from 1116x759, update the `width`/`height` attributes on the
  `<img>` in `docs/index.html`.

## Capture log (v0.9.0 working tree, `tauri dev` debug build, real library on this PC)

All images are 1270x760 PNG (WebView client area, grabbed with PrintWindow, no pixel edits) in `docs/ph/`. Real state only:
the one pending update (War Thunder, Steam) was genuinely pending at capture time. No names, emails or user paths visible.
Caveat: captured from `tauri dev`, not the release build the plan asks for; re-shoot from the shipped build if that matters.

Produced:
- Shot 1 hero: `docs/ph/01-hero-updates.png` (also copied to `docs/screenshot.png`; landing `<img>` set to 1270x760).
- Shot 2 unknown: `docs/ph/02-unknown-not-guess.png`. Full window, not a close crop: GOG "Up to date" next to Ubisoft Connect "Unknown" rows. Crop at edit time if wanted.
- Shot 5 privacy: `docs/ph/05-first-run-privacy.png`. Real first-run welcome dialog (profile storage temporarily set aside so it showed once, then restored). Some launcher/game icons behind the dialog had not drawn yet in that fresh profile.
- Shot 7 light theme: `docs/ph/07-light-theme.png` (light theme ships in v0.9.0).
- Extra: `docs/ph/04-confirm-before-update-all.png` (Update all confirm dialog, cancelled, nothing sent).
- Extra: `docs/ph/06-settings-privacy.png` (Settings: all background options off by default, "Nothing is sent anywhere").

Skipped:
- Shot 3 before/after: needs real launcher windows tiled on the desktop; not staged to avoid launching launchers/games.
- Shot 4 truth table: a designed graphic, not a screenshot; not produced here.
- Shot 6 loop: needs a real Update click (would start a real download); not done.
- Error row and app-update banner shots: no such state exists on this machine right now. (Steam's real "2.0 TB" total is visible in shots 01 and 07.)
- The plan's fallback "Launch button" shot is covered by 01/02/07 (Launch buttons visible on Steam, GOG, Ubisoft rows).
