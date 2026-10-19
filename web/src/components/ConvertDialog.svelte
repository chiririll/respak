<script lang="ts">
  import { api, type TargetInfo } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { basketRequests, store } from '../lib/store.svelte';
  import { categoryColor, categoryOf, errorText } from '../lib/util';
  import OptionsForm from './OptionsForm.svelte';

  let { onclose }: { onclose: () => void } = $props();

  let targets = $state<TargetInfo[]>([]);
  let loading = $state(true);
  let chosen = $state('');
  let options = $state<Record<string, unknown>>({});
  let invalid = $state(false);
  let filter = $state<Record<string, string>>({});
  let outPath = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  const rows = $derived(store.basket);
  const single = $derived(rows.length === 1 && (rows[0].kind === 'file' || rows[0].kind === 'asset'));
  const known = $derived(new Set(targets.map((t) => t.type)));
  const others = $derived(
    (store.session?.formats ?? []).filter((f) => !known.has(f.name) && f.name !== 'file'),
  );
  const current = $derived(targets.find((t) => t.type === chosen));
  const sampled = $derived(rows.filter((r) => r.kind === 'file' || r.kind === 'asset').length);

  // The targets of the files in the basket: what can be done with all of them (or any, for a mix).
  $effect(() => {
    let live = true;
    (async () => {
      const files = rows.filter((r) => r.kind === 'file' || r.kind === 'asset').slice(0, 8);
      const found = await Promise.all(files.map((r) => api.entry(r.path).catch(() => null)));
      const merged = new Map<string, { info: TargetInfo; count: number }>();
      for (const entry of found) {
        for (const t of entry?.targets ?? []) {
          const slot = merged.get(t.type);
          if (slot) slot.count++;
          else merged.set(t.type, { info: t, count: 1 });
        }
      }
      if (!live) return;
      targets = [...merged.values()].sort((a, b) => b.count - a.count).map((m) => m.info);
      if (!chosen || !known.has(chosen)) chosen = targets[0]?.type ?? '';
    })().finally(() => live && (loading = false));
    return () => {
      live = false;
    };
  });

  $effect(() => {
    if (current) options = { ...current.defaults };
    else options = {};
  });

  async function start() {
    error = null;
    if (!chosen) return;
    busy = true;
    try {
      const to = { type: chosen, ...options };
      const items = basketRequests(rows, to, filter);
      if (single && outPath.trim()) items[0].path = outPath.trim();
      const job = await api.convert(items);
      store.jobs = [...store.jobs.filter((j) => j.id !== job.id), job];
      store.panel = 'jobs';
      store.clearBasket();
      onclose();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  const needsFilter = (kind: string) => kind === 'dir' || kind === 'container' || kind === 'folder';

  function onkey(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onclick={onclose}>
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-modal="true" aria-label="Convert">
    <header>
      <h2>Convert</h2>
      <button class="btn icon sm ghost" onclick={onclose} aria-label="Close"><Icon name="x" size={16} /></button>
    </header>

    <div class="content">
      <section>
        <h3>From <span class="chip">{rows.length}</span></h3>
        <ul class="rows">
          {#each rows as r (r.key)}
            {@const cat = categoryOf(r.kind === 'group' ? (r.ext ?? '') : r.name, r.kind)}
            <li>
              <span style:color={categoryColor[cat]}>
                <Icon name={r.kind === 'dir' || r.kind === 'folder' ? 'folder' : r.kind === 'container' ? 'package' : 'file'} size={15} />
              </span>
              <span class="n tail" title={r.path}><bdi dir="ltr">{r.kind === 'group' ? `all ${r.ext} in ${r.path}` : r.path || '/'}</bdi></span>
              {#if needsFilter(r.kind)}
                <input class="kind" type="text" placeholder="kind, e.g. txtr (all)" bind:value={filter[r.key]} />
              {/if}
              <button class="btn icon sm ghost" onclick={() => store.untick(r.key)} aria-label="Remove"><Icon name="x" size={13} /></button>
            </li>
          {:else}
            <li class="empty">The basket is empty.</li>
          {/each}
        </ul>
      </section>

      <section>
        <h3>To</h3>
        {#if loading}
          <div class="muted"><span class="spin"><Icon name="refresh" size={14} /></span> Looking at what these can become…</div>
        {:else}
          <div class="targets">
            {#each targets as t (t.type)}
              <button class="target" class:on={chosen === t.type} onclick={() => (chosen = t.type)} title={t.label}>
                <span class="tt">{t.type}</span>
                {#if t.label !== t.type}<span class="tl">{t.label}</span>{/if}
              </button>
            {/each}
          </div>
          {#if others.length}
            <details class="other" open={targets.length === 0}>
              <summary>{targets.length ? 'Other formats' : 'Choose a format'}</summary>
              <div class="targets">
                {#each others as f (f.name)}
                  <button
                    class="target"
                    class:on={chosen === f.name}
                    onclick={() => {
                      chosen = f.name;
                    }}
                    title={f.label}
                  >
                    <span class="tt">{f.name}</span>
                    {#if f.label !== f.name}<span class="tl">{f.label}</span>{/if}
                  </button>
                {/each}
              </div>
            </details>
          {/if}
          {#if sampled < rows.length && targets.length}
            <p class="hint"><Icon name="info" size={13} /> Folders and paks are not looked into: a file that cannot become {chosen || 'the target'} is reported as a failure, the others still convert.</p>
          {/if}
        {/if}
      </section>

      {#if chosen}
        <section>
          <h3>Options <span class="chip accent">{chosen}</span></h3>
          {#if current}
            <OptionsForm defaults={current.defaults} bind:value={options} bind:invalid />
          {:else}
            <p class="muted">Default options.</p>
          {/if}
          {#if single}
            <label class="field out">
              <span class="k">Output path <span class="muted">(optional, under the output folder)</span></span>
              <input type="text" placeholder="default" bind:value={outPath} />
            </label>
          {/if}
        </section>
      {/if}
    </div>

    <footer>
      {#if error}<span class="err"><Icon name="alert" size={14} /> {error}</span>{/if}
      <span class="spacer"></span>
      <span class="muted dest mono" title="Where results are written">→ {store.session?.out}</span>
      <button class="btn" onclick={onclose}>Cancel</button>
      <button class="btn primary" disabled={!chosen || invalid || busy || rows.length === 0} onclick={start}>
        {#if busy}<span class="spin"><Icon name="refresh" size={14} /></span>{:else}<Icon name="bolt" size={14} />{/if}
        Convert {rows.length} {rows.length === 1 ? 'item' : 'items'}
      </button>
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
    z-index: 50;
    padding: 24px;
  }
  .dialog {
    width: min(720px, 100%);
    max-height: min(780px, 100%);
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
  h3 {
    margin: 0 0 10px;
    font-size: 11.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .content {
    overflow: auto;
    padding: 6px 18px 18px;
    display: grid;
    gap: 22px;
    align-content: start;
  }
  .content > section:first-child {
    margin-top: 12px;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 190px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--bg);
  }
  .rows li {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 5px 8px 5px 12px;
    border-bottom: 1px solid var(--line);
  }
  .rows li:last-child {
    border-bottom: 0;
  }
  .rows .empty {
    color: var(--muted);
    padding: 14px;
  }
  .n {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .kind {
    width: 190px;
    height: 28px;
    flex: none;
  }
  .targets {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 8px;
  }
  .target {
    text-align: left;
    background: var(--bg);
    border: 1px solid var(--line-2);
    border-radius: 10px;
    padding: 9px 12px;
    display: grid;
    gap: 1px;
    transition: border-color 0.12s, background 0.12s;
  }
  .target:hover {
    border-color: var(--faint);
  }
  .target.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .tt {
    font-weight: 700;
  }
  .tl {
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .other {
    margin-top: 12px;
  }
  .other summary {
    cursor: pointer;
    color: var(--muted);
    margin-bottom: 8px;
    font-weight: 600;
  }
  .hint {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    color: var(--muted);
    font-size: 12.5px;
    margin: 10px 0 0;
  }
  .field.out {
    display: grid;
    gap: 5px;
    margin-top: 14px;
  }
  .k {
    font-weight: 600;
    font-size: 12.5px;
    color: var(--text-2);
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
  .err {
    color: var(--err);
    display: inline-flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
  }
  .dest {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
