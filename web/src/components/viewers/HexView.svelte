<script lang="ts">
  import { api, type HexView } from '../../lib/api';
  import { errorText, formatSize } from '../../lib/util';

  let { path }: { path: string } = $props();

  let data = $state<HexView | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    let live = true;
    data = null;
    error = null;
    api
      .hex(path)
      .then((d) => live && (data = d))
      .catch((e) => live && (error = errorText(e)));
    return () => {
      live = false;
    };
  });
</script>

<div class="hex">
  {#if error}
    <div class="note err">{error}</div>
  {:else if data}
    <div class="note">First {formatSize(Math.min(1024, data.size))} of {formatSize(data.size)}</div>
    <div class="rows mono">
      {#each data.rows as row}
        <div class="r"><span class="o">{row.offset}</span><span class="h">{row.hex}</span><span class="a">{row.ascii}</span></div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .hex {
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 14px 16px;
    overflow: auto;
    max-height: 420px;
  }
  .note {
    color: var(--muted);
    font-size: 12.5px;
    margin-bottom: 8px;
  }
  .note.err {
    color: var(--err);
  }
  .r {
    display: grid;
    grid-template-columns: 72px 400px 1fr;
    gap: 12px;
    white-space: pre;
    line-height: 1.6;
  }
  .o {
    color: var(--faint);
  }
  .h {
    color: var(--text-2);
  }
  .a {
    color: var(--accent);
  }
</style>
