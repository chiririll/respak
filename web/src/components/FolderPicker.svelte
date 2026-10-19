<script lang="ts">
  import { api, type FsListing } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { errorText } from '../lib/util';

  let {
    start = '',
    title = 'Choose a folder',
    onpick,
    onclose,
  }: { start?: string; title?: string; onpick: (path: string) => void; onclose: () => void } = $props();

  let listing = $state<FsListing | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let typed = $state('');

  async function go(path: string, fallback = false) {
    loading = true;
    error = null;
    try {
      listing = await api.fs(path);
      typed = listing.path;
    } catch (e) {
      // The starting path may not exist yet (an output folder): begin at the roots instead.
      if (fallback && !listing) return go('');
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void go(start, true);
  });

  function onkey(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onclick={onclose}>
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="btn icon sm ghost" onclick={onclose} aria-label="Close"><Icon name="x" size={16} /></button>
    </header>

    <form class="where" onsubmit={(e) => { e.preventDefault(); void go(typed); }}>
      <button type="button" class="btn icon" disabled={listing?.parent == null} onclick={() => go(listing?.parent ?? '')} title="Up"><Icon name="up" size={16} /></button>
      <input type="text" bind:value={typed} spellcheck="false" placeholder="Type a path and press Enter" />
    </form>

    {#if listing?.shortcuts}
      <div class="shortcuts">
        {#if listing.shortcuts.home}<button class="btn sm" onclick={() => go(listing!.shortcuts.home!)}><Icon name="home" size={13} /> Home</button>{/if}
        <button class="btn sm" onclick={() => go(listing!.shortcuts.cwd)}><Icon name="folder" size={13} /> Working folder</button>
        <button class="btn sm" onclick={() => go('')}><Icon name="hdd" size={13} /> Drives</button>
      </div>
    {/if}

    <div class="list">
      {#if error}
        <div class="state err"><Icon name="alert" size={16} /> {error}</div>
      {:else if loading && !listing}
        <div class="state"><span class="spin"><Icon name="refresh" size={16} /></span></div>
      {:else if listing}
        {#each listing.dirs as d (d.path)}
          <button class="dir" ondblclick={() => go(d.path)} onclick={() => go(d.path)}>
            <Icon name="folder" size={16} />
            <span>{d.name}</span>
          </button>
        {:else}
          <div class="state">No subfolders.</div>
        {/each}
      {/if}
    </div>

    <footer>
      {#if listing && listing.path}
        <span class="info muted">
          {#if listing.containers}<span class="chip ok">{listing.containers} paks here</span>{:else}{listing.files ?? 0} files{/if}
        </span>
      {/if}
      <span class="spacer"></span>
      <button class="btn" onclick={onclose}>Cancel</button>
      <button class="btn primary" disabled={!listing?.path} onclick={() => onpick(listing!.path)}>Use this folder</button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    backdrop-filter: blur(3px);
    display: grid;
    place-items: center;
    z-index: 60;
    padding: 24px;
  }
  .dialog {
    width: min(560px, 100%);
    height: min(600px, 100%);
    display: flex;
    flex-direction: column;
    background: var(--bg-2);
    border: 1px solid var(--line-2);
    border-radius: 16px;
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  .where {
    display: flex;
    gap: 8px;
    padding: 12px 18px 8px;
  }
  .shortcuts {
    display: flex;
    gap: 6px;
    padding: 0 18px 10px;
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 4px 10px;
    border-top: 1px solid var(--line);
  }
  .dir {
    display: flex;
    gap: 10px;
    align-items: center;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    color: var(--text-2);
    padding: 7px 10px;
    border-radius: 8px;
  }
  .dir:hover {
    background: var(--hover);
    color: var(--text);
  }
  .state {
    padding: 30px;
    text-align: center;
    color: var(--muted);
    display: flex;
    gap: 8px;
    justify-content: center;
  }
  .state.err {
    color: var(--err);
  }
  footer {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 12px 18px;
    border-top: 1px solid var(--line);
    background: var(--bg-3);
  }
  .spacer {
    flex: 1;
  }
</style>
