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

If this ever changes, it'll be in a release's changelog, not buried in an
update.
