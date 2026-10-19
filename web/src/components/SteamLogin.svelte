<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { errorText } from '../lib/util';

  let account = $state<{ loggedIn: boolean; accountName?: string } | null>(null);
  let method = $state<'qr' | 'password'>('qr');
  let running = $state(false);
  let username = $state('');
  let password = $state('');
  let code = $state('');
  let error = $state<string | null>(null);

  const login = $derived(store.steamLogin);
  const qr = $derived(login?.state === 'qr' ? login : null);
  const needs = $derived(login?.state === 'needsCode' ? login : null);

  async function refresh() {
    try {
      account = await api.steamStatus();
    } catch {
      /* the page shows no account */
    }
  }

  onMount(() => {
    void refresh();
    api.steamLoginState().then((s) => (running = s.running)).catch(() => {});
  });

  // A login ends with `done` or `failed`; its start is ours.
  $effect(() => {
    if (login?.state === 'done' || login?.state === 'failed') {
      running = false;
      password = '';
      code = '';
      if (login.state === 'failed') error = login.message;
      else void refresh();
    }
  });

  async function start() {
    error = null;
    store.steamLogin = null;
    try {
      if (method === 'qr') await api.steamLoginQr();
      else await api.steamLoginCredentials(username, password);
      running = true;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function submitCode() {
    error = null;
    try {
      await api.steamLoginCode(code);
      code = '';
    } catch (e) {
      error = errorText(e);
    }
  }

  async function cancel() {
    try {
      await api.steamLoginCancel();
    } catch (e) {
      error = errorText(e);
    }
    running = false;
    password = '';
    store.steamLogin = null;
  }

  async function logout() {
    try {
      await api.steamLogout();
      store.steamLogin = null;
      await refresh();
    } catch (e) {
      store.fail(e);
    }
  }

  /** The QR code as one SVG path of dark modules, with a quiet zone. */
  function path(size: number, dark: boolean[]): string {
    let d = '';
    for (let y = 0; y < size; y++) {
      let x = 0;
      while (x < size) {
        if (!dark[y * size + x]) {
          x++;
          continue;
        }
        let run = 1;
        while (x + run < size && dark[y * size + x + run]) run++;
        d += `M${x + 4} ${y + 4}h${run}v1h-${run}z`;
        x += run;
      }
    }
    return d;
  }
</script>

<div class="box" class:ok={account?.loggedIn}>
  <div class="head">
    <Icon name="steam" size={18} />
    <div class="t">
      {#if account?.loggedIn}
        <b>Signed in as {account.accountName}</b>
        <span class="muted">The token stays on this computer, in <span class="mono">.respak/</span>.</span>
      {:else}
        <b>Sign in to Steam</b>
        <span class="muted">Needed once; the token is kept in <span class="mono">.respak/account.json</span> and never shown here.</span>
      {/if}
    </div>
    {#if account?.loggedIn}
      <button class="btn sm" onclick={logout}>Sign out</button>
    {/if}
  </div>

  {#if !account?.loggedIn}
    <div class="seg" role="tablist">
      <button role="tab" aria-selected={method === 'qr'} class:on={method === 'qr'} disabled={running} onclick={() => (method = 'qr')}>QR code</button>
      <button role="tab" aria-selected={method === 'password'} class:on={method === 'password'} disabled={running} onclick={() => (method = 'password')}>Account name &amp; password</button>
    </div>

    {#if method === 'qr'}
      {#if qr}
        <div class="qr">
          <svg viewBox="0 0 {qr.qr.size + 8} {qr.qr.size + 8}" role="img" aria-label="Steam login QR code" shape-rendering="crispEdges">
            <rect width={qr.qr.size + 8} height={qr.qr.size + 8} fill="#fff" />
            <path d={path(qr.qr.size, qr.qr.dark)} fill="#000" />
          </svg>
          <ol>
            <li>Open the <b>Steam</b> mobile app</li>
            <li>Go to <b>Steam Guard</b> → <b>Sign in with QR</b> (or the + button)</li>
            <li>Scan this code and approve</li>
          </ol>
        </div>
      {:else if running}
        <div class="wait"><span class="spin"><Icon name="refresh" size={15} /></span> Asking Steam for a code…</div>
      {/if}
    {:else}
      <div class="creds">
        <label>
          <span class="k">Account name</span>
          <input type="text" bind:value={username} autocomplete="username" spellcheck="false" disabled={running} />
        </label>
        <label>
          <span class="k">Password</span>
          <input type="password" bind:value={password} autocomplete="current-password" disabled={running} />
        </label>
        <p class="note"><Icon name="info" size={13} /> The password goes to this local server for this sign-in only. It is not stored or logged.</p>
      </div>

      {#if needs}
        <div class="guard">
          {#if needs.retry}<div class="err"><Icon name="alert" size={14} /> Steam rejected that code. Try again.</div>{/if}
          {#if needs.kind}
            <label>
              <span class="k">{needs.kind === 'email' ? 'Code emailed to you' : 'Authenticator code'}</span>
              <form class="row" onsubmit={(e) => { e.preventDefault(); void submitCode(); }}>
                <input type="text" bind:value={code} inputmode="text" autocomplete="one-time-code" spellcheck="false" maxlength="10" />
                <button class="btn primary" type="submit" disabled={!code.trim()}>Send</button>
              </form>
            </label>
          {/if}
          {#if needs.confirmable}
            <div class="wait"><span class="spin"><Icon name="refresh" size={15} /></span> {needs.kind ? 'Or approve' : 'Approve'} the sign-in in the Steam mobile app…</div>
          {/if}
        </div>
      {/if}
    {/if}

    {#if error}<div class="err"><Icon name="alert" size={14} /> {error}</div>{/if}

    <div class="actions">
      {#if running}
        <button class="btn" onclick={cancel}>Cancel</button>
      {:else}
        <button class="btn primary" onclick={start} disabled={method === 'password' && (!username.trim() || !password)}>
          {method === 'qr' ? 'Show QR code' : 'Sign in'}
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .box {
    display: grid;
    gap: 12px;
    padding: 14px;
    border: 1px dashed var(--line-2);
    border-radius: 12px;
  }
  .box.ok {
    border-style: solid;
    border-color: color-mix(in srgb, var(--ok) 40%, transparent);
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: center;
    color: var(--muted);
  }
  .box.ok .head :global(.icon) {
    color: var(--ok);
  }
  .t {
    flex: 1;
    display: grid;
    min-width: 0;
  }
  .t b {
    color: var(--text);
  }
  .seg {
    display: inline-flex;
    justify-self: start;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 9px;
    padding: 2px;
    gap: 2px;
  }
  .seg button {
    border: 0;
    background: transparent;
    color: var(--muted);
    padding: 4px 12px;
    border-radius: 7px;
    font-weight: 600;
    font-size: 13px;
  }
  .seg button.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .qr {
    display: flex;
    gap: 20px;
    align-items: center;
    flex-wrap: wrap;
  }
  .qr svg {
    width: 200px;
    height: 200px;
    border-radius: 10px;
    border: 1px solid var(--line-2);
    flex: none;
  }
  ol {
    margin: 0;
    padding-left: 18px;
    color: var(--text-2);
    display: grid;
    gap: 6px;
  }
  .creds {
    display: grid;
    gap: 12px;
    grid-template-columns: 1fr 1fr;
  }
  label {
    display: grid;
    gap: 5px;
  }
  .k {
    font-weight: 600;
    font-size: 12.5px;
  }
  .note {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--muted);
    font-size: 12.5px;
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .guard {
    display: grid;
    gap: 10px;
    padding: 12px;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 10px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .wait {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
  }
  .err {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    color: var(--err);
    word-break: break-word;
  }
  .actions {
    display: flex;
  }
  @media (max-width: 560px) {
    .creds {
      grid-template-columns: 1fr;
    }
  }
</style>
