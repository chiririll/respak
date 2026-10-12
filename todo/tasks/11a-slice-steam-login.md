# 11a · Slice: Steam login
**Status:** todo · **Depends on:** 11 · **See:** [STEAM.md](../STEAM.md) §1–4

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

## Tests (proposed, to agree at the gate)
- Token file: write/read round trip, missing file, malformed file; logout deletes it.
- Expiry check from a synthetic JWT (`exp` past, near, far).
- QR rotation: a fake poll sequence (pending → new URL → pending → token) redraws once and returns the token.
- `cargo build -p respak-cli --no-default-features` has no `respak-steam` in `cargo tree`.
- Live (env var, the user's account, never CI): token logon succeeds after a fresh login.

## Demo
The user runs `steam login --qr`, scans the code, then opens a new shell, where a command that needs Steam logs on without a prompt.

## To verify
Refresh token lifetime and the renewal call; that a logon with the refresh token in `access_token` works through steamroom (STEAM.md §9).
