# Privacy

LaunchPilot collects nothing and sends nothing.

- **No accounts.** There's no sign-in, no server-side user record.
- **No telemetry, no analytics.** The app doesn't phone home. There is no
  event tracking, no crash reporting service, no usage metrics of any kind.
- **No network calls except the two that are the whole point:** checking a
  launcher's own public update endpoint (e.g. GOG's `content-system.gog.com`)
  to read version info, and checking `github.com/vxkShelby/Launch-Pilot`
  for a newer LaunchPilot release. Neither sends anything about you or your
  library — they're plain unauthenticated GET/HEAD requests.
- **Everything else is read locally.** Installed games, launcher state, and
  update status all come from files and registry keys already on your own
  machine (the same ones each launcher reads itself).
- **Crash logs stay on your machine.** If LaunchPilot crashes, one line is
  appended to `%APPDATA%\LaunchPilot\crash.log`. That file never leaves your
  computer unless you attach it yourself to a bug report.
- **Optional background features (all off by default).** In Settings you can
  turn on a tray icon (the app keeps running after you close the window),
  update notifications, and start-with-Windows. Turning them on stores two
  flags in `%APPDATA%\LaunchPilot\settings.json`; start-with-Windows adds a
  `LaunchPilot` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  (plus the `StartupApproved` flag Windows manages itself), and turning it off
  removes it. Notifications are local Windows toasts showing a game name from
  your own library; nothing is sent anywhere. With the tray on, the periodic
  update check (including the check for a newer LaunchPilot release) can run
  while the window is hidden.

If this ever changes, it'll be in a release's changelog, not buried in an
update.
