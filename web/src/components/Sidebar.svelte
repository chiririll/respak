<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api, type SearchResult } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { TreeModel } from '../lib/tree.svelte';
  import { basename, categoryColor, categoryOf, dirname, errorText, formatCount } from '../lib/util';
  import Tree from './Tree.svelte';

  const model = new TreeModel();

  let query = $state('');
  let scope = $state<'all' | 'files' | 'loaded'>('all');
  let result = $state<SearchResult | null>(null);
  let searching = $state(false);
  let error = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout>;
  let serial = 0;

  function run() {
    clearTimeout(timer);
    if (!query.trim()) {
      result = null;
      searching = false;
      error = null;
      return;
    }
    searching = true;
    timer = setTimeout(async () => {
      const mine = ++serial;
      try {
        const found = await api.search(query, scope);
        if (mine === serial) {
          result = found;
          error = null;
        }
      } catch (e) {
        if (mine === serial) error = errorText(e);
      } finally {
        if (mine === serial) searching = false;
      }
    }, 220);
  }

  function open(path: string) {
    store.select(path, true);
  }

  let seenSearch = store.searchRequest.tick;
  $effect(() => {
    const { q, tick } = store.searchRequest;
    if (tick === seenSearch) return;
    seenSearch = tick;
    query = q;
    scope = 'loaded';
    run();
  });

  onMount(() => {
    void model.start();
  });
  onDestroy(() => clearTimeout(timer));

  const scopes = [
    ['all', 'Everything'],
    ['files', 'File names'],
    ['loaded', 'Opened paks'],
  ] as const;
</script>

<aside class="side">
  <div class="search">
    <div class="field">
      <Icon name="search" size={15} />
      <input
        type="search"
        placeholder="Search names…"
        bind:value={query}
        oninput={run}
        spellcheck="false"
        autocomplete="off"
      />
      {#if query}
        <button class="clear" onclick={() => { query = ''; run(); }} aria-label="Clear"><Icon name="x" size={14} /></button>
      {/if}
    </div>
    {#if query}
      <div class="scopes">
        {#each scopes as [id, label]}
          <button class="scope" class:on={scope === id} onclick={() => { scope = id; run(); }}>{label}</button>
        {/each}
      </div>
    {/if}
  </div>

  {#if query}
    <div class="results">
      {#if error}
        <div class="state err">{error}</div>
      {:else if searching && !result}
        <div class="state"><span class="spin"><Icon name="refresh" size={14} /></span> Searching…</div>
      {:else if result}
        <div class="summary">
          {formatCount(result.hits.length)}{result.truncated ? '+' : ''} matches
          {#if scope !== 'files'}<span class="faint">· {result.containers} opened {result.containers === 1 ? 'pak' : 'paks'} searched</span>{/if}
        </div>
        {#each result.hits as hit (hit.kind + hit.path)}
          {@const cat = categoryOf(hit.name, hit.kind === 'asset' ? 'asset' : 'file')}
          <button class="hit" class:selected={store.selected === hit.path} onclick={() => open(hit.path)}>
            <span style:color={categoryColor[cat]}><Icon name={hit.kind === 'asset' ? 'file' : 'file'} size={15} /></span>
            <span class="txt">
              <span class="n">{hit.name}</span>
              <span class="p tail"><bdi dir="ltr">{hit.container ?? dirname(hit.path) ?? ''}</bdi></span>
            </span>
          </button>
        {:else}
          <div class="state">
            Nothing found.
            {#if scope !== 'files'}<br /><span class="faint">Only opened paks are searched for assets: expand a pak first.</span>{/if}
          </div>
        {/each}
      {/if}
    </div>
  {:else}
    <Tree {model} />
  {/if}
</aside>

<style>
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    background: var(--bg-2);
    border-right: 1px solid var(--line);
  }
  .search {
    padding: 12px;
    border-bottom: 1px solid var(--line);
    display: grid;
    gap: 8px;
  }
  .field {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--muted);
  }
  .field :global(.icon) {
    position: absolute;
    left: 12px;
    pointer-events: none;
  }
  .field input {
    padding-left: 34px;
    padding-right: 32px;
  }
  .clear {
    position: absolute;
    right: 6px;
    border: 0;
    background: transparent;
    color: var(--muted);
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border-radius: 6px;
  }
  .clear:hover {
    background: var(--hover);
    color: var(--text);
  }
  .scopes {
    display: flex;
    gap: 6px;
  }
  .scope {
    border: 1px solid var(--line-2);
    background: transparent;
    color: var(--muted);
    border-radius: 99px;
    padding: 2px 10px;
    font-size: 12px;
    font-weight: 550;
  }
  .scope:hover {
    color: var(--text);
  }
  .scope.on {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }
  .results {
    flex: 1;
    overflow: auto;
    padding: 6px;
  }
  .summary {
    padding: 4px 8px 8px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .faint {
    color: var(--faint);
  }
  .hit {
    display: flex;
    gap: 10px;
    align-items: center;
    width: 100%;
    text-align: left;
    padding: 6px 8px;
    border-radius: 8px;
    border: 0;
    background: transparent;
    color: var(--text-2);
  }
  .hit:hover {
    background: var(--hover);
  }
  .hit.selected {
    background: var(--sel);
  }
  .txt {
    display: grid;
    min-width: 0;
  }
  .n {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .p {
    color: var(--faint);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .state {
    padding: 24px 12px;
    text-align: center;
    color: var(--muted);
  }
  .state.err {
    color: var(--err);
  }
</style>
