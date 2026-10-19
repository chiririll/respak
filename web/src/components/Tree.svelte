<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { TreeModel, type Flat, type Row } from '../lib/tree.svelte';
  import { categoryColor, categoryOf, formatCount, formatSize } from '../lib/util';

  const ROW = 30;
  const OVERSCAN = 12;

  let { model }: { model: TreeModel } = $props();

  let viewport: HTMLDivElement;
  let height = $state(600);
  let top = $state(0);

  const total = $derived(model.flat.length);
  const first = $derived(Math.max(0, Math.floor(top / ROW) - OVERSCAN));
  const last = $derived(Math.min(total, Math.ceil((top + height) / ROW) + OVERSCAN));
  const slice = $derived(model.flat.slice(first, last));

  function iconFor(row: Row) {
    const { kind, name } = row.node;
    const cat = categoryOf(kind === 'group' ? (row.node.ext ?? name) : name, kind);
    const icon =
      kind === 'dir' || kind === 'folder'
        ? row.expanded
          ? 'folder-open'
          : 'folder'
        : (
            { container: 'package', image: 'image', audio: 'audio', video: 'film', model: 'cube', text: 'text', table: 'table', other: 'file', dir: 'folder' } as const
          )[cat];
    return { icon, color: categoryColor[cat] };
  }

  function activate(row: Row) {
    const { kind, path } = row.node;
    if (kind === 'dir' || kind === 'folder' || kind === 'group') {
      void model.toggle(row);
      return;
    }
    store.select(path);
    if (kind === 'container') void model.toggle(row);
  }

  function tickable(row: Row): boolean {
    if (row.node.kind === 'folder') return !row.container()?.names;
    return true;
  }

  function ticked(row: Row): boolean {
    return store.isTicked(row.node.path, row.node.kind, row.node.ext);
  }

  function tickRow(row: Row) {
    store.toggleTick({
      path: row.node.path,
      kind: row.node.kind,
      ext: row.node.ext,
      name: row.node.kind === 'group' ? `${row.node.name} · ${row.container()?.node.name ?? ''}` : row.node.name || '/',
    });
  }

  function rowKey(f: Flat): string {
    return f.type === 'row' ? `r:${f.row.node.kind}:${f.row.node.path}:${f.row.node.ext ?? ''}` : `${f.type}:${f.row.node.path}:${f.row.node.kind}`;
  }

  function onKey(e: KeyboardEvent) {
    const flat = model.flat;
    const current = flat.findIndex((f) => f.type === 'row' && f.row.node.path === store.selected);
    const rows = (i: number) => {
      for (let j = i; j >= 0 && j < flat.length; j += i < current ? -1 : 1) if (flat[j].type === 'row') return j;
      return -1;
    };
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = rows(current + (e.key === 'ArrowDown' ? 1 : -1));
      const f = flat[next];
      if (f?.type === 'row') {
        store.select(f.row.node.path);
        scrollTo(next);
      }
    } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      const f = flat[current];
      if (f?.type !== 'row' || !f.row.expandable) return;
      if ((e.key === 'ArrowRight') !== f.row.expanded) void model.toggle(f.row);
    }
  }

  function scrollTo(index: number) {
    const y = index * ROW;
    if (y < top + ROW) viewport.scrollTop = Math.max(0, y - ROW);
    else if (y > top + height - ROW * 2) viewport.scrollTop = y - height + ROW * 2;
  }

  /** Opens the selected path in the tree, centred. */
  async function reveal() {
    const path = store.selected;
    if (!path) return;
    const row = await model.reveal(path);
    if (!row) return;
    await tick();
    const index = model.indexOf(row);
    if (index >= 0) viewport.scrollTop = Math.max(0, index * ROW - height / 2);
  }

  let lastReveal = store.revealTick;
  $effect(() => {
    if (store.revealTick !== lastReveal) {
      lastReveal = store.revealTick;
      void reveal();
    }
  });

  onMount(() => {
    const observer = new ResizeObserver(() => (height = viewport.clientHeight));
    observer.observe(viewport);
    height = viewport.clientHeight;
    return () => observer.disconnect();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="viewport"
  bind:this={viewport}
  onscroll={() => (top = viewport.scrollTop)}
  onkeydown={onKey}
  role="tree"
  tabindex="0"
  aria-label="Files"
>
  <div class="spacer" style:height="{total * ROW}px">
    <div class="window" style:transform="translateY({first * ROW}px)">
      {#each slice as f (rowKey(f))}
        {#if f.type === 'row'}
          {@const row = f.row}
          {@const { icon, color } = iconFor(row)}
          {@const selected = store.selected === row.node.path && row.node.kind !== 'group' && row.node.kind !== 'folder' && row.node.kind !== 'dir'}
          <div
            class="row"
            class:selected
            class:ticked={ticked(row)}
            style:padding-left="{8 + row.depth * 16}px"
            role="treeitem"
            aria-selected={selected}
            aria-expanded={row.expandable ? row.expanded : undefined}
            onclick={() => activate(row)}
            onkeydown={(e) => e.key === 'Enter' && activate(row)}
            tabindex="-1"
          >
            <span class="chev" class:open={row.expanded} class:hidden={!row.expandable}>
              {#if row.loading}
                <span class="spin"><Icon name="refresh" size={12} /></span>
              {:else}
                <Icon name="chevron" size={12} />
              {/if}
            </span>
            {#if tickable(row)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="tick" onclick={(e) => e.stopPropagation()}>
                <input type="checkbox" checked={ticked(row)} onchange={() => tickRow(row)} aria-label="Add to basket" />
              </span>
            {:else}
              <span class="tick"></span>
            {/if}
            <span class="ico" style:color><Icon name={icon} size={16} /></span>
            <span class="name" title={row.node.path}>{row.node.name}</span>

            {#if row.node.kind === 'container' && row.expanded}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="tools" onclick={(e) => e.stopPropagation()}>
                <button class="seg" class:on={row.view === 'path'} onclick={() => model.setView(row, 'path')} title="Paths, as the pak lists them">Paths</button>
                <button class="seg" class:on={row.view === 'kind'} onclick={() => model.setView(row, 'kind')} title="Group by asset kind">Kinds</button>
                <button
                  class="seg"
                  class:on={row.names}
                  disabled={row.names || row.loading}
                  onclick={() => model.readNames(row)}
                  title="Read every entry to learn its name (slow)"
                >
                  Names
                </button>
              </span>
            {/if}

            <span class="meta">
              {#if row.node.count !== undefined}
                {formatCount(row.node.count)}
              {:else if row.node.size !== undefined && row.node.kind !== 'dir'}
                {formatSize(row.node.size)}
              {/if}
            </span>
          </div>
        {:else if f.type === 'more'}
          <button class="more" style:margin-left="{8 + (f.row.depth + 1) * 16}px" onclick={() => model.load(f.row, true)} disabled={f.row.loading}>
            {#if f.row.loading}<span class="spin"><Icon name="refresh" size={12} /></span>{/if}
            Show more ({formatCount(f.row.total - (f.row.children?.length ?? 0))} left)
          </button>
        {:else}
          <div class="note" class:error={f.text !== 'empty'} style:padding-left="{36 + (f.row.depth + 1) * 16}px">{f.text}</div>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .viewport {
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
    position: relative;
  }
  .spacer {
    position: relative;
  }
  .window {
    position: absolute;
    inset: 0 0 auto 0;
  }
  .row,
  .more,
  .note {
    height: 30px;
    display: flex;
    align-items: center;
  }
  .row {
    gap: 6px;
    padding-right: 12px;
    color: var(--text-2);
    cursor: pointer;
    position: relative;
    user-select: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.ticked .name {
    color: var(--text);
  }
  .row.selected {
    background: var(--sel);
    color: var(--text);
  }
  .row.selected::before {
    content: '';
    position: absolute;
    left: 0;
    top: 4px;
    bottom: 4px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: linear-gradient(var(--accent), var(--accent-2));
  }
  .chev {
    width: 14px;
    display: grid;
    place-items: center;
    color: var(--faint);
    transition: transform 0.12s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .chev.hidden {
    visibility: hidden;
  }
  .tick {
    width: 16px;
    display: grid;
    place-items: center;
    opacity: 0;
    transition: opacity 0.1s;
  }
  .row:hover .tick,
  .row.ticked .tick,
  .row:focus-within .tick {
    opacity: 1;
  }
  input[type='checkbox'] {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .ico {
    display: grid;
    place-items: center;
  }
  .name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .meta {
    flex: none;
  }
  .meta {
    color: var(--faint);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    padding-left: 12px;
  }
  .tools {
    flex: none;
    display: inline-flex;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 7px;
    overflow: hidden;
  }
  .seg {
    border: 0;
    background: transparent;
    color: var(--muted);
    font-size: 11.5px;
    font-weight: 600;
    padding: 2px 8px;
  }
  .seg:hover:not(:disabled) {
    color: var(--text);
  }
  .seg.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .seg:disabled {
    cursor: default;
  }
  .more {
    border: 0;
    background: transparent;
    color: var(--accent);
    font-size: 12.5px;
    font-weight: 600;
    gap: 6px;
    padding: 0 8px;
  }
  .more:hover {
    text-decoration: underline;
  }
  .note {
    color: var(--faint);
    font-size: 12.5px;
    font-style: italic;
  }
  .note.error {
    color: var(--err);
    font-style: normal;
  }
</style>
