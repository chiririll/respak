<script lang="ts">
  import { api, type Config, type Resolved } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { errorText } from '../lib/util';
  import FolderPicker from './FolderPicker.svelte';
  import SteamLogin from './SteamLogin.svelte';
  import VersionPicker from './VersionPicker.svelte';

  type Source = 'root' | 'steam';

  const cfg = $derived(store.configState);
  let source = $state<Source>('root');
  let root = $state('');
  let out = $state('out');
  let cache = $state('');
  let search = $state('');
  let noPatches = $state(false);
  let rig = $state('');
  let studio = $state('');
  let app = $state('');
  let depots = $state('');
  let branch = $state('');
  let cacheSize = $state('');
  let verbose = $state(false);
  let picker = $state<null | 'root' | 'out' | 'cache'>(null);
  let busy = $state<null | 'save' | 'connect'>(null);
  let problem = $state<string | null>(null);
  let pickVersion = $state(false);
  let loaded = false;

  function fill(config: Config) {
    source = config.steam && !config.root ? 'steam' : 'root';
    root = config.root ?? '';
    out = config.out ?? '';
    cache = config.cache ?? '';
    search = (config.search ?? []).join('\n');
    noPatches = config.no_patches ?? false;
    rig = config.respawn?.rig ?? '';
    studio = config.respawn?.studio ?? '';
    app = config.steam ? String(config.steam.app) : '';
    depots = (config.steam?.depots ?? []).join('\n');
    branch = config.steam?.branch ?? '';
    cacheSize = config.steam?.cache_size ?? '';
    verbose = config.steam?.verbose ?? false;
  }

  $effect(() => {
    if (cfg && !loaded) {
      loaded = true;
      fill(cfg.config);
    }
  });

  const lines = (text: string) => text.split('\n').map((l) => l.trim()).filter(Boolean);
  const opt = (text: string) => (text.trim() ? text.trim() : undefined);

  function build(): Config {
    const config: Config = {};
    if (source === 'root') {
      config.root = opt(root);
    } else {
      config.steam = {
        app: Number(app) || 0,
        depots: lines(depots).map((d) => (/^\d+$/.test(d) ? Number(d) : d)),
        branch: opt(branch),
        cache_size: opt(cacheSize),
        verbose: verbose || undefined,
      };
    }
    config.out = opt(out);
    config.cache = opt(cache);
    const globs = lines(search);
    if (globs.length) config.search = globs;
    if (noPatches) config.no_patches = true;
    if (opt(rig) || opt(studio)) config.respawn = { rig: opt(rig), studio: opt(studio) };
    return config;
  }

  const ready = $derived(
    out.trim() !== '' && (source === 'root' ? root.trim() !== '' : Number(app) > 0 && lines(depots).length > 0),
  );

  async function save(connect: boolean) {
    busy = connect ? 'connect' : 'save';
    problem = null;
    try {
      const saved = await api.saveConfig(build());
      await store.refreshConfig();
      if (connect) {
        if (saved.problem) throw new Error(saved.problem);
        await store.connect();
      } else {
        store.toast('success', `Saved ${saved.file}`);
        problem = saved.problem;
      }
    } catch (e) {
      problem = errorText(e);
    } finally {
      busy = null;
    }
  }

  function usePinned(resolved: Resolved) {
    app = String(resolved.app);
    depots = resolved.depots.join('\n');
    pickVersion = false;
    store.toast('success', `${resolved.depots.length} depot${resolved.depots.length === 1 ? '' : 's'} pinned`);
  }

  function picked(path: string) {
    if (picker === 'root') root = path;
    else if (picker === 'out') out = path;
    else if (picker === 'cache') cache = path;
    picker = null;
  }
</script>

<div class="page">
  <div class="card">
    <div class="hero">
      <div class="logo"><Icon name="cube" size={26} /></div>
      <div>
        <h1>Where are the game files?</h1>
        <p>Pick a folder on this computer, or read them straight from Steam. Settings are kept in <span class="mono">{cfg?.file ?? 'respak.toml'}</span>, the same file the command line uses.</p>
      </div>
    </div>

    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={source === 'root'} class:on={source === 'root'} onclick={() => (source = 'root')}>
        <Icon name="folder" size={17} />
        <span><b>Local folder</b><small>Game directory on disk</small></span>
      </button>
      <button
        role="tab"
        aria-selected={source === 'steam'}
        class:on={source === 'steam'}
        disabled={!cfg?.steamAvailable}
        onclick={() => (source = 'steam')}
        title={cfg?.steamAvailable ? '' : 'This build has no Steam support'}
      >
        <Icon name="steam" size={17} />
        <span><b>Steam</b><small>Download only what you open</small></span>
      </button>
    </div>

    {#if source === 'root'}
      <label class="field">
        <span class="k">Game folder</span>
        <div class="row">
          <input type="text" bind:value={root} placeholder="C:\Games\Apex Legends" spellcheck="false" />
          <button class="btn" onclick={() => (picker = 'root')}><Icon name="folder-open" size={15} /> Browse</button>
        </div>
      </label>
    {:else}
      <div class="version">
        <button class="vbtn" onclick={() => (pickVersion = !pickVersion)} aria-expanded={pickVersion}>
          <Icon name="tag" size={16} />
          <span><b>Pick a game version</b><small>Seasons and events, instead of typing manifest ids</small></span>
          <span class="vchev" class:open={pickVersion}><Icon name="chevron" size={14} /></span>
        </button>
        {#if pickVersion}<div class="vbody"><VersionPicker onpick={usePinned} /></div>{/if}
      </div>

      <div class="grid2">
        <label class="field">
          <span class="k">App ID</span>
          <input type="text" inputmode="numeric" bind:value={app} placeholder="1172470" />
        </label>
        <label class="field">
          <span class="k">Branch <span class="opt">optional</span></span>
          <input type="text" bind:value={branch} placeholder="public" />
        </label>
      </div>
      <label class="field">
        <span class="k">Depots <span class="opt">one per line: ID or ID=MANIFEST</span></span>
        <textarea rows="3" class="mono" bind:value={depots} placeholder={'1172471\n1172472=7843921000000000000'}></textarea>
      </label>
      <SteamLogin />
    {/if}

    <label class="field">
      <span class="k">Output folder <span class="opt">converted files go here</span></span>
      <div class="row">
        <input type="text" bind:value={out} placeholder="out" spellcheck="false" />
        <button class="btn" onclick={() => (picker = 'out')}><Icon name="folder-open" size={15} /> Browse</button>
      </div>
    </label>

    <details class="more">
      <summary>More options</summary>
      <div class="inner">
        <label class="field">
          <span class="k">Search globs <span class="opt">where to look for paks; one per line</span></span>
          <textarea rows="2" class="mono" bind:value={search} placeholder="paks/Win64/*.rpak"></textarea>
        </label>
        <label class="field">
          <span class="k">Cache folder <span class="opt">plugin cache, e.g. the guid → pak index</span></span>
          <div class="row">
            <input type="text" bind:value={cache} spellcheck="false" />
            <button class="btn" onclick={() => (picker = 'cache')}><Icon name="folder-open" size={15} /> Browse</button>
          </div>
        </label>
        <div class="grid2">
          <label class="field">
            <span class="k">Rig <span class="opt">auto, GUID or path</span></span>
            <input type="text" bind:value={rig} placeholder="auto" />
          </label>
          <label class="field">
            <span class="k">Studio layout <span class="opt">auto, v16, v17…</span></span>
            <input type="text" bind:value={studio} placeholder="auto" />
          </label>
        </div>
        {#if source === 'steam'}
          <div class="grid2">
            <label class="field">
              <span class="k">Steam chunk cache size</span>
              <input type="text" bind:value={cacheSize} placeholder="8GiB" />
            </label>
            <label class="field sw">
              <span class="k">Log every chunk request</span>
              <input type="checkbox" class="switch" bind:checked={verbose} />
            </label>
          </div>
        {/if}
        <label class="field sw">
          <span class="k">Ignore rpak patches <span class="opt">do not read patch_master.rpak</span></span>
          <input type="checkbox" class="switch" bind:checked={noPatches} />
        </label>
      </div>
    </details>

    {#if problem}
      <div class="problem"><Icon name="alert" size={16} /> <span>{problem}</span></div>
    {/if}

    <div class="actions">
      {#if store.session}
        <button class="btn ghost" onclick={() => (store.page = 'explorer')}>Back to files</button>
      {/if}
      <span class="spacer"></span>
      <button class="btn" disabled={!ready || busy !== null} onclick={() => save(false)}>
        {#if busy === 'save'}<span class="spin"><Icon name="refresh" size={14} /></span>{/if} Save
      </button>
      <button class="btn primary" disabled={!ready || busy !== null} onclick={() => save(true)}>
        {#if busy === 'connect'}<span class="spin"><Icon name="refresh" size={14} /></span>{:else}<Icon name="plug" size={15} />{/if}
        {busy === 'connect' && source === 'steam' ? 'Connecting to Steam…' : 'Save & connect'}
      </button>
    </div>
  </div>
</div>

{#if picker}
  <FolderPicker
    title={picker === 'root' ? 'Game folder' : picker === 'out' ? 'Output folder' : 'Cache folder'}
    start={(picker === 'root' ? root : picker === 'out' ? out : cache).trim() || (cfg?.cwd ?? '')}
    onpick={picked}
    onclose={() => (picker = null)}
  />
{/if}

<style>
  .page {
    height: 100%;
    overflow: auto;
    display: grid;
    place-items: start center;
    padding: 48px 20px 80px;
    background:
      radial-gradient(900px 400px at 15% -10%, color-mix(in srgb, var(--accent) 12%, transparent), transparent 70%),
      radial-gradient(700px 400px at 95% 0%, color-mix(in srgb, var(--accent-2) 9%, transparent), transparent 70%);
  }
  .card {
    width: min(680px, 100%);
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 20px;
    padding: 28px;
    display: grid;
    gap: 20px;
    box-shadow: var(--shadow);
  }
  .hero {
    display: flex;
    gap: 16px;
    align-items: flex-start;
  }
  .logo {
    flex: none;
    width: 52px;
    height: 52px;
    border-radius: 14px;
    display: grid;
    place-items: center;
    color: #fff;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: 0 10px 26px -10px var(--accent);
  }
  h1 {
    margin: 0 0 4px;
    font-size: 22px;
    letter-spacing: -0.01em;
  }
  p {
    margin: 0;
    color: var(--muted);
  }
  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .tabs button {
    display: flex;
    gap: 12px;
    align-items: center;
    text-align: left;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 12px;
    padding: 12px 14px;
    color: var(--muted);
    transition: border-color 0.12s, background 0.12s;
  }
  .tabs button span {
    display: grid;
  }
  .tabs button b {
    color: var(--text);
  }
  .tabs button small {
    color: var(--muted);
    font-size: 12px;
  }
  .tabs button:hover:not(:disabled) {
    border-color: var(--faint);
  }
  .tabs button.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
  }
  .tabs button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .field {
    display: grid;
    gap: 6px;
  }
  .k {
    font-weight: 600;
    font-size: 13px;
  }
  .opt {
    color: var(--faint);
    font-weight: 450;
    margin-left: 4px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  .version {
    border: 1px solid var(--line);
    border-radius: 12px;
    overflow: hidden;
  }
  .vbtn {
    display: flex;
    gap: 12px;
    align-items: center;
    width: 100%;
    text-align: left;
    background: var(--bg);
    border: 0;
    padding: 11px 14px;
    color: var(--accent);
  }
  .vbtn span:nth-child(2) {
    flex: 1;
    display: grid;
  }
  .vbtn b {
    color: var(--text);
  }
  .vbtn small {
    color: var(--muted);
    font-size: 12px;
  }
  .vchev {
    color: var(--faint);
    transition: transform 0.12s;
    display: grid;
  }
  .vchev.open {
    transform: rotate(90deg);
  }
  .vbody {
    padding: 12px 14px 14px;
    border-top: 1px solid var(--line);
  }
  .more {
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 10px 14px;
  }
  .more summary {
    cursor: pointer;
    font-weight: 600;
    color: var(--text-2);
  }
  .inner {
    display: grid;
    gap: 16px;
    margin-top: 16px;
  }
  .field.sw {
    grid-template-columns: 1fr auto;
    align-items: center;
  }
  .switch {
    appearance: none;
    width: 38px;
    height: 22px;
    border-radius: 99px;
    background: var(--line-2);
    position: relative;
    cursor: pointer;
    transition: background 0.15s;
    margin: 0;
  }
  .switch::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::after {
    transform: translateX(16px);
  }
  .problem {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 14px;
    border-radius: 10px;
    color: var(--err);
    background: color-mix(in srgb, var(--err) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--err) 30%, transparent);
    word-break: break-word;
  }
  .actions {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .spacer {
    flex: 1;
  }
  @media (max-width: 560px) {
    .tabs,
    .grid2 {
      grid-template-columns: 1fr;
    }
  }
</style>
