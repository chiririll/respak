# 26 · Slice: Steam in the web UI
**Status:** todo (spec done; sketch and gate pending) · **Depends on:** 23 · **See:** [webui.md](../design/webui.md) §2 step 1, §5, [steam.md](../design/steam.md) §3-§4

## Goal
The settings page can log in to Steam (QR, or username and password with Steam Guard), show the login state, and connect to a depot input; the explorer then works over Steam as it does over a folder.

## Changes
1. Needs login flows callable without a terminal: `respak-steam` exposes the QR flow as a stream of challenge URLs (redrawn on `new_challenge_url`) and the credentials flow as steps (needs-guard-code, needs-mobile-confirmation, done). This is the only change outside `respak-web`; Steam code stays in `respak-steam`.
2. API: `GET /api/steam/status`, `POST /api/steam/login/qr` (SSE of challenges until done), `POST /api/steam/login/credentials` and `POST /api/steam/login/code`, `POST /api/steam/logout`. The token is never returned.
3. The password is held only for the login call, never logged or stored; the form says so.
4. Frontend: login panel (QR rendered in the page), state indicator, depot fields (`ID` / `ID=MANIFEST`, branch).
5. One Steam session per account: the web session owns the single connection; a second login while one exists is refused with a clear message.
6. Download statistics (chunks, bytes, cache hits) shown in the UI.

## Leaves out
The version catalog (27). Depot listing from PICS. Branch passwords.

## Open
- Whether the credentials form is shown by default or behind "use password".

## Proposed tests (for the gate)
- Synthetic login-flow driver: QR challenge refresh, guard-code step, mobile-confirmation step, expiry and "run login" error map to the right API states.
- The API never returns the refresh token or the password (response bodies checked).
- Logout deletes `.respak/account.json`.
- Live Steam tests: `#[ignore]` and `RESPAK_STEAM_LIVE=1`, **only with the user's explicit permission** (steam.md §8).
