<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { formatSize } from '../lib/util';

  const s = $derived(store.session);
  const where = $derived(s ? (s.source === 'root' ? s.root : `Steam · app ${s.steam?.app}`) : null);
</script>

<header class="top">
  <div class="brand">
    <span class="mark"><Icon name="cube" size={18} /></span>
    <span class="word">respak</span>
  </div>

  {#if s}
    <button class="source" onclick={() => (store.page = 'settings')} title="Change the source">
      <Icon name={s.source === 'root' ? 'folder' : 'steam'} size={15} />
      <span class="w mono">{where}</span>
      <span class="live" class:on={store.live}></span>
    </button>
    {#if store.steamStats}
      <span class="chip" title="Steam chunks downloaded / served from the cache">
        <Icon name="download" size={12} />
        {formatSize(store.steamStats.bytesDownloaded)} · {store.steamStats.cacheHits} cached
      </span>
    {/if}
  {/if}

  <span class="spacer"></span>

  {#if store.progress}
    <span class="prog" title={store.progress.task}>
      <span class="spin"><Icon name="refresh" size={13} /></span>
      <span class="pt">{store.progress.task}</span>
      <span class="mono">{store.progress.done}/{store.progress.total}</span>
    </span>
  {/if}

  {#if s}
    <button class="btn sm ghost" class:active={store.panel === 'jobs'} onclick={() => (store.panel = store.panel === 'jobs' ? 'none' : 'jobs')}>
      <Icon name="jobs" size={15} /> Jobs
      {#if store.running}<span class="badge">{store.running}</span>{:else if store.jobs.length}<span class="badge quiet">{store.jobs.length}</span>{/if}
    </button>
    <button class="btn sm ghost" onclick={() => (store.page = 'settings')}><Icon name="settings" size={15} /> Settings</button>
    <button class="btn sm ghost" onclick={() => store.disconnect().catch((e) => store.fail(e))} title="Disconnect"><Icon name="plug" size={15} /></button>
  {/if}

  <button
    class="btn icon sm ghost"
    onclick={() => store.setTheme(store.theme === 'dark' ? 'light' : 'dark')}
    title="Switch theme"
    aria-label="Switch theme"
  >
    <Icon name={store.theme === 'dark' ? 'sun' : 'moon'} size={16} />
  </button>
</header>

<style>
  .top {
    height: 56px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 14px 0 18px;
    border-bottom: 1px solid var(--line);
    background: var(--bg-2);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .mark {
    width: 30px;
    height: 30px;
    border-radius: 9px;
    display: grid;
    place-items: center;
    color: #fff;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: 0 6px 16px -6px var(--accent);
  }
  .word {
    font-weight: 750;
    font-size: 17px;
    letter-spacing: -0.02em;
  }
  .source {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 46vw;
    height: 32px;
    padding: 0 12px;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 99px;
    color: var(--text-2);
  }
  .source:hover {
    border-color: var(--faint);
  }
  .w {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .live {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--faint);
    flex: none;
  }
  .live.on {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 25%, transparent);
  }
  .spacer {
    flex: 1;
  }
  .prog {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
    max-width: 280px;
  }
  .pt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .btn.active {
    background: var(--hover);
    color: var(--text);
  }
  .badge {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 99px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #fff;
    font-size: 11px;
    font-weight: 700;
    display: grid;
    place-items: center;
  }
  .badge.quiet {
    background: var(--bg-4);
    color: var(--muted);
  }
</style>
