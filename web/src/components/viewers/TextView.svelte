<script lang="ts">
  import { api, type EntryView } from '../../lib/api';
  import Icon from '../../lib/Icon.svelte';
  import { store } from '../../lib/store.svelte';
  import { errorText } from '../../lib/util';

  let { path, entry }: { path: string; entry: EntryView } = $props();

  let target = $state('');
  let text = $state('');
  let error = $state<string | null>(null);
  let loading = $state(false);

  $effect(() => {
    target = entry.textTargets[0] ?? '';
  });

  $effect(() => {
    if (!target) return;
    let live = true;
    loading = true;
    error = null;
    api
      .text(path, target)
      .then((t) => live && (text = t))
      .catch((e) => {
        if (live) {
          error = errorText(e);
          text = '';
        }
      })
      .finally(() => live && (loading = false));
    return () => {
      live = false;
    };
  });

  const pretty = $derived.by(() => {
    if (!target.endsWith('json')) return text;
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  });

  async function copy() {
    try {
      await navigator.clipboard.writeText(pretty);
      store.toast('success', 'Copied', 1500);
    } catch {
      store.toast('error', 'Could not copy');
    }
  }
</script>

<div class="wrap">
  <div class="bar">
    {#if entry.textTargets.length > 1}
      <select bind:value={target} aria-label="Format">
        {#each entry.textTargets as t}<option value={t}>{t}</option>{/each}
      </select>
    {:else}
      <span class="chip">{target}</span>
    {/if}
    <span class="spacer"></span>
    <button class="btn sm" onclick={copy} disabled={!pretty}><Icon name="copy" size={14} /> Copy</button>
  </div>
  <div class="code">
    {#if loading}
      <div class="state"><span class="spin"><Icon name="refresh" size={16} /></span> Converting…</div>
    {:else if error}
      <div class="state err"><Icon name="alert" size={16} /> {error}</div>
    {:else}
      <pre>{pretty}</pre>
    {/if}
  </div>
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .bar {
    display: flex;
    gap: 10px;
    align-items: center;
    padding-bottom: 12px;
  }
  select {
    width: auto;
    min-width: 160px;
  }
  .spacer {
    flex: 1;
  }
  .code {
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 12px;
  }
  pre {
    margin: 0;
    padding: 16px;
    white-space: pre;
    color: var(--text-2);
    tab-size: 2;
  }
  .state {
    padding: 20px;
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
  }
  .state.err {
    color: var(--err);
  }
</style>
