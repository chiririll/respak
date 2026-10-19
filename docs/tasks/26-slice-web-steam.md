# 26 · Slice: Steam in the web UI
**Status:** in-progress (built; never run against Steam) · **Depends on:** 23 · **See:** [webui.md](../design/webui.md) §2 step 1, §5, [steam.md](../design/steam.md) §3-§4

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

## Built
`respak-steam` gained `login_qr_with` (a callback gets the challenge URL on every rotation), `qr_matrix`, and `login_user_with` (Steam Guard events and a code future; `login_user` for the CLI is built on it, behaviour unchanged). `respak-web` turns them into SSE events (`steamLogin`: `qr`, `needsCode`, `done`, `failed`) and takes the code on `POST /api/steam/login/code`; `GET /api/steam/status`, `POST /api/steam/logout` (deletes `.respak/account.json`), `DELETE /api/steam/login`. The password lives in the one flow that uses it and is never logged, stored or answered with; one login at a time (a second is a 409). The QR code is drawn in the page from the module matrix.
- Not run: no live Steam test was run (steam.md §8: only with the user's explicit permission). Tested: the matrix, the event shapes, the refusals; the flows themselves are the CLI's, refactored.
- Download statistics are shown in the top bar once the Steam input has served chunks.
- Left as before: depot listing from PICS, branch passwords.
