<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type Catalog, type CatalogGame, type CatalogVersion, type Resolved } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { errorText } from '../lib/util';

  let { onpick }: { onpick: (resolved: Resolved) => void } = $props();

  let catalog = $state<Catalog | null>(null);
  let error = $state<string | null>(null);
  let gameKey = $state('');
  let query = $state('');
  let chosen = $state<CatalogVersion | null>(null);
  let depots = $state<Record<number, boolean>>({});
  let result = $state<Resolved | null>(null);
  let busy = $state(false);

  const game = $derived<CatalogGame | undefined>(catalog?.games.find((g) => g.key === gameKey));
  const visible = $derived(
    (game?.versions ?? []).filter((v) => {
      const q = query.trim().toLowerCase();
      return !q || v.name.toLowerCase().includes(q) || v.id.includes(q) || v.date.includes(q) || (v.group ?? '').includes(q);
    }),
  );
  // Newest first; versions of one group sit under one heading.
  const groups = $derived.by(() => {
    const out: { name: string | null; versions: CatalogVersion[] }[] = [];
    for (const v of visible) {
      const last = out[out.length - 1];
      if (last && last.name === v.group) last.versions.push(v);
      else out.push({ name: v.group, versions: [v] });
    }
    return out;
  });

  onMount(async () => {
    try {
      catalog = await api.catalog();
      gameKey = catalog.games[0]?.key ?? '';
    } catch (e) {
      error = errorText(e);
    }
  });

  $effect(() => {
    // The game's default depots are ticked when it changes.
    const g = game;
    if (!g) return;
    const marked = g.depots.some((d) => d.default);
    depots = Object.fromEntries(g.depots.map((d) => [d.id, marked ? d.default : true]));
    chosen = null;
    result = null;
  });

  async function pick(version: CatalogVersion) {
    chosen = version;
    await resolve();
  }

  async function resolve() {
    if (!game || !chosen) return;
    busy = true;
    error = null;
    try {
      const wanted = game.depots.filter((d) => depots[d.id]).map((d) => d.id);
      result = await api.resolveVersion(game.key, chosen.id, wanted.length ? wanted : undefined);
    } catch (e) {
      error = errorText(e);
      result = null;
    } finally {
      busy = false;
    }
  }

  const typeLabel = { season: 'Season', event: 'Event', hotfix: 'Hotfix' } as const;
</script>

<div class="picker">
  {#if error}
    <div class="err"><Icon name="alert" size={14} /> {error}</div>
  {/if}

  {#if catalog && catalog.games.length}
    <div class="games">
      {#each catalog.games as g (g.key)}
        <button class:on={gameKey === g.key} onclick={() => (gameKey = g.key)}>{g.name}</button>
      {/each}
    </div>

    {#if game && game.versions.length}
      <input type="search" placeholder="Search versions, e.g. season 20" bind:value={query} />
      <div class="list">
        {#each groups as g}
          {#if g.name}<div class="group">{g.name}</div>{/if}
          {#each g.versions as v (v.id)}
            <button class="ver" class:on={chosen?.id === v.id} onclick={() => pick(v)}>
              <span class="n">{v.name}</span>
              {#if v.type}<span class="chip">{typeLabel[v.type]}</span>{/if}
              <span class="d mono">{v.date}</span>
            </button>
          {/each}
        {:else}
          <div class="empty">No version matches.</div>
        {/each}
      </div>

      {#if game.depots.length > 1}
        <div class="depots">
          {#each game.depots as d (d.id)}
            <label class="dep">
              <input type="checkbox" bind:checked={depots[d.id]} onchange={resolve} />
              <span>{d.name ?? 'Depot'} <span class="mono muted">{d.id}</span></span>
            </label>
          {/each}
        </div>
      {/if}

      {#if result}
        <div class="result">
          <div class="rt"><b>{result.depots.length}</b> {result.depots.length === 1 ? 'depot' : 'depots'} pinned at <b>{chosen?.name}</b></div>
          <div class="pins mono">{result.depots.join('  ')}</div>
          {#if result.leftOut.length}
            <div class="warn"><Icon name="alert" size={13} /> No manifest is known for depot {result.leftOut.join(', ')} at this version, so {result.leftOut.length === 1 ? 'it is' : 'they are'} left out.</div>
          {/if}
          <button class="btn primary sm" disabled={busy || result.depots.length === 0} onclick={() => onpick(result!)}>
            <Icon name="check" size={14} /> Use this version
          </button>
        </div>
      {/if}
    {:else if game}
      <div class="empty">
        <b>{game.name}</b> has no versions yet.
        <span class="muted">Add a file to <span class="mono">{catalog.userDir ?? '.respak/catalog'}</span> (see the comments in the shipped catalog for the format), or type the depots below.</span>
      </div>
    {/if}

    {#each catalog.problems as p}
      <div class="err"><Icon name="alert" size={14} /> {p.message}</div>
    {/each}
  {:else if catalog}
    <div class="empty">The catalog is empty.</div>
  {:else if !error}
    <div class="empty"><span class="spin"><Icon name="refresh" size={14} /></span></div>
  {/if}
</div>

<style>
  .picker {
    display: grid;
    gap: 10px;
  }
  .games {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .games button {
    border: 1px solid var(--line-2);
    background: transparent;
    color: var(--muted);
    border-radius: 99px;
    padding: 3px 12px;
    font-weight: 600;
    font-size: 13px;
  }
  .games button.on {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }
  .list {
    max-height: 230px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--bg);
    padding: 4px;
  }
  .group {
    padding: 8px 10px 2px;
    font-size: 11.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
    font-weight: 700;
  }
  .ver {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    color: var(--text-2);
    padding: 6px 10px;
    border-radius: 7px;
  }
  .ver:hover {
    background: var(--hover);
  }
  .ver.on {
    background: var(--sel);
    color: var(--text);
  }
  .n {
    flex: 1;
    min-width: 0;
  }
  .d {
    color: var(--faint);
  }
  .depots {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
  }
  .dep {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: 13px;
  }
  .dep input {
    accent-color: var(--accent);
  }
  .result {
    display: grid;
    gap: 8px;
    justify-items: start;
    padding: 12px;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 10px;
  }
  .pins {
    color: var(--text-2);
    word-break: break-all;
  }
  .warn {
    display: flex;
    gap: 8px;
    color: var(--warn);
    font-size: 12.5px;
  }
  .err {
    display: flex;
    gap: 8px;
    color: var(--err);
    word-break: break-word;
  }
  .empty {
    color: var(--muted);
    padding: 8px 2px;
    display: grid;
    gap: 4px;
  }
</style>
