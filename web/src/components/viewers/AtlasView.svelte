<script lang="ts">
  import { api, blobUrl, type Atlas, type AtlasSprite } from '../../lib/api';
  import Icon from '../../lib/Icon.svelte';
  import { errorText } from '../../lib/util';
  import Stage from './Stage.svelte';

  let { path }: { path: string } = $props();

  let url = $state<string | null>(null);
  let atlas = $state<Atlas | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let picked = $state<AtlasSprite | null>(null);
  let hovered = $state<number | null>(null);
  let filter = $state('');

  $effect(() => {
    const abort = new AbortController();
    loading = true;
    error = null;
    atlas = null;
    picked = null;
    // The server converts once and keeps the result for the second request.
    api
      .atlas(path)
      .then(async (a) => {
        atlas = a;
        const next = await blobUrl(api.atlasImageUrl(path), abort.signal);
        const old = url;
        url = next;
        if (old) URL.revokeObjectURL(old);
      })
      .catch((e) => {
        if (!abort.signal.aborted) error = errorText(e);
      })
      .finally(() => {
        if (!abort.signal.aborted) loading = false;
      });
    return () => abort.abort();
  });

  const sprites = $derived(
    (atlas?.sprites ?? []).filter((s) => !filter.trim() || s.path.toLowerCase().includes(filter.trim().toLowerCase())),
  );
</script>

<div class="wrap">
  <div class="main">
    <Stage {url} {loading} {error}>
      {#snippet overlay({ scale, w, h })}
        {@const k = atlas && atlas.width ? w / atlas.width : 1}
        {#each atlas?.sprites ?? [] as s (s.index)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="sprite"
            class:picked={picked?.index === s.index}
            class:hot={hovered === s.index}
            style:left="{s.x * k}px"
            style:top="{s.y * k}px"
            style:width="{s.width * k}px"
            style:height="{s.height * k}px"
            style:border-width="{Math.max(1, 1.5 / scale)}px"
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() => (picked = s)}
            onmouseenter={() => (hovered = s.index)}
            onmouseleave={() => (hovered = null)}
            title={s.path}
          ></div>
        {/each}
      {/snippet}
    </Stage>
  </div>

  <div class="list">
    <div class="head">
      <span>Sprites</span>
      <span class="chip">{atlas?.sprites.length ?? 0}</span>
    </div>
    <input type="search" placeholder="Filter sprites…" bind:value={filter} />
    <div class="items">
      {#each sprites as s (s.index)}
        <button
          class="item"
          class:on={picked?.index === s.index}
          onclick={() => (picked = s)}
          onmouseenter={() => (hovered = s.index)}
          onmouseleave={() => (hovered = null)}
        >
          <span class="p">{s.path || `sprite ${s.index}`}</span>
          <span class="d mono">{s.width}×{s.height}</span>
        </button>
      {/each}
    </div>
    {#if picked}
      <div class="detail mono">
        <div><b>{picked.path}</b></div>
        <div>x {picked.x}, y {picked.y}</div>
        <div>{picked.width}×{picked.height} px</div>
      </div>
    {/if}
  </div>
</div>

<style>
  .wrap {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 260px;
    gap: 14px;
    flex: 1;
    min-height: 0;
  }
  .main {
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .sprite {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, var(--accent) 55%, transparent);
    cursor: pointer;
  }
  .sprite.hot {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-color: var(--accent);
  }
  .sprite.picked {
    background: color-mix(in srgb, var(--accent) 30%, transparent);
    border-color: #fff;
    z-index: 2;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 12px;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: 650;
  }
  .items {
    flex: 1;
    overflow: auto;
    margin: 0 -6px;
  }
  .item {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    border: 0;
    background: transparent;
    padding: 5px 8px;
    border-radius: 7px;
    text-align: left;
    color: var(--text-2);
  }
  .item:hover {
    background: var(--hover);
  }
  .item.on {
    background: var(--sel);
    color: var(--text);
  }
  .p {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .d {
    color: var(--faint);
    flex: none;
  }
  .detail {
    border-top: 1px solid var(--line);
    padding-top: 10px;
    color: var(--text-2);
    word-break: break-all;
  }
  @media (max-width: 1100px) {
    .wrap {
      grid-template-columns: 1fr;
    }
    .list {
      max-height: 260px;
    }
  }
</style>
