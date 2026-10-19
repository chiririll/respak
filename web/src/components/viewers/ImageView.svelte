<script lang="ts">
  import { api, blobUrl, type EntryView } from '../../lib/api';
  import Icon from '../../lib/Icon.svelte';
  import { errorText } from '../../lib/util';
  import Stage from './Stage.svelte';

  let { path, entry }: { path: string; entry: EntryView } = $props();

  const mips = $derived(Math.max(1, entry.bitmap?.mips ?? 1));
  let mip = $state(0);
  let url = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let size = $state<{ w: number; h: number } | null>(null);

  $effect(() => {
    path;
    mip = 0;
  });

  $effect(() => {
    const target = api.imageUrl(path, mip);
    const abort = new AbortController();
    loading = true;
    error = null;
    blobUrl(target, abort.signal)
      .then((u) => {
        const old = url;
        url = u;
        if (old) URL.revokeObjectURL(old);
      })
      .catch((e) => {
        if (abort.signal.aborted) return;
        error = errorText(e);
        url = null;
      })
      .finally(() => {
        if (!abort.signal.aborted) loading = false;
      });
    return () => abort.abort();
  });

  function dims(level: number): string {
    const b = entry.bitmap;
    if (!b) return '';
    return `${Math.max(1, b.width >> level)}×${Math.max(1, b.height >> level)}`;
  }
</script>

<div class="wrap">
  <div class="bar">
    {#if entry.bitmap}
      <div class="mips" role="group" aria-label="Mip level">
        <span class="lbl">Mip</span>
        {#each Array(mips) as _, i}
          <button class:on={mip === i} onclick={() => (mip = i)} title={dims(i)}>{i}</button>
        {/each}
      </div>
      <span class="dim mono">{dims(mip)}</span>
    {:else if size}
      <span class="dim mono">{size.w}×{size.h}</span>
    {/if}
    {#if (entry.bitmap?.layers ?? 1) > 1}
      <span class="chip" title="Only the first layer is previewed">{entry.bitmap?.layers} layers · layer 0 shown</span>
    {/if}
  </div>

  <Stage {url} {loading} {error} onsize={(w, h) => (size = { w, h })} />
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 0 12px;
    flex-wrap: wrap;
  }
  .mips {
    display: inline-flex;
    align-items: center;
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: 9px;
    padding: 2px;
    gap: 2px;
  }
  .lbl {
    color: var(--muted);
    font-size: 12px;
    padding: 0 8px 0 8px;
    font-weight: 600;
  }
  .mips button {
    border: 0;
    background: transparent;
    color: var(--muted);
    min-width: 28px;
    height: 26px;
    border-radius: 7px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .mips button:hover {
    color: var(--text);
  }
  .mips button.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .dim {
    color: var(--muted);
  }
</style>
