# 11a · Slice: Steam login
**Status:** done · **Depends on:** 11 · **See:** [STEAM.md](../STEAM.md) §1–4

## Goal
Log in to Steam once; later runs reuse the stored token without asking again:
```
respak steam login --qr            # or --user NAME
respak steam logout
```

## Scope
**respak-steam** (new crate, host only):
- `steamroom` pinned to a git rev, plus `qrcode` and `rpassword` (STEAM.md §2).
- QR login. The poll loop is ours, so a `new_challenge_url` redraws the QR code.
- Credential login with Steam Guard: email code, authenticator code, or confirmation in the mobile app.
- `.respak/account.json` = `{ account_name, steam_id, refresh_token }`. Logon with the stored token; check its expiry (`exp`); renew it once; otherwise "run `respak steam login`".

**respak-cli:**
- Cargo feature `steam` (on by default) → optional dependency on `respak-steam`.
- `steam login --qr | --user NAME`, `steam logout`.
- `--no-default-features` builds without any Steam code.

**Repo:** `.respak/` in `.gitignore`.

## Leaves out
Depots, manifests, chunks, the cache (11b). `--steam` as an input.

## Tests (taken as agreed: "go" at the gate)
- Token file: write/read round trip, missing file, malformed file; logout deletes it.
- Expiry check from a synthetic JWT (`exp` past, near, far).
- QR rotation: a fake poll sequence (pending → new URL → pending → token) redraws once and returns the token.
- `cargo build -p respak-cli --no-default-features` has no `respak-steam` in `cargo tree`.
- Live (env var, the user's account, never CI): token logon succeeds after a fresh login.

## Demo
The user runs `steam login --qr`, scans the code, then opens a new shell, where a command that needs Steam logs on without a prompt.

## To verify
Refresh token lifetime and the renewal call; that a logon with the refresh token in `access_token` works through steamroom (STEAM.md §9).

## Done
- `respak-steam`: `account.rs` (token file, JWT `exp`/`sub`), `qr.rs` (our poll loop behind a `Poller` trait, rendering), `login.rs` (QR; credentials with Steam Guard code and mobile confirmation polled concurrently), `session.rs` (connect TCP→WS, logon with the refresh token in `access_token`, renewal), steamroom pinned to rev `4dd310c`.
- Renewal: `Authentication.GenerateAccessTokenForApp#1` (`renewal_type = 1`) through `call_service_method_non_authed`, because steamroom has no wrapper. Near expiry (<24 h): renew, but go on with the old token if renewal fails. Expired, or rejected by Steam: renew once, else "run `respak steam login`".
- CLI: feature `steam` (default); `steam login --qr | --user`, `steam logout`. `--root`/`--out` became optional in `Cli` (required by every command but `steam`; checked in `main`).
- Tests: token file (round trip, missing, malformed, logout), expiry (past/near/far), QR rotation (redraws once), CLI parsing; `--no-default-features` has no Steam code in `cargo tree`. Live: `RESPAK_STEAM_LIVE=1 cargo test -p respak-steam --test live` after a login.
- Demo passed (QR login by the user, then token logon from a fresh process via the live test).
- Learned: the refresh token lives ~212 days (`exp - iat`), its audience includes `renew`; the QR code rotates about every 30 s; a refresh token in `access_token` logs on through steamroom (STEAM.md §9, both verified).
- Still unverified live: the renewal call (`GenerateAccessTokenForApp`, not reachable before ~190 days or a forced expiry) and a password + Steam Guard login.
