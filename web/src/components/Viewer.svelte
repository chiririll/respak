<script lang="ts">
  import { api, type EntryView } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { basename, categoryColor, categoryOf, errorText, formatSize } from '../lib/util';
  import AtlasView from './viewers/AtlasView.svelte';
  import AudioView from './viewers/AudioView.svelte';
  import HexView from './viewers/HexView.svelte';
  import ImageView from './viewers/ImageView.svelte';
  import TextView from './viewers/TextView.svelte';

  let { path }: { path: string } = $props();

  let entry = $state<EntryView | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  $effect(() => {
    let live = true;
    loading = true;
    error = null;
    api
      .entry(path)
      .then((e) => live && (entry = e))
      .catch((e) => {
        if (live) {
          error = errorText(e);
          entry = null;
        }
      })
      .finally(() => live && (loading = false));
    return () => {
      live = false;
    };
  });

  const name = $derived(basename(path));
  const crumbs = $derived(path.split('/'));
  const cat = $derived(categoryOf(name, 'file'));
  // What ticking this entry stands for: a container is everything inside it; a file or an asset
  // is itself (an asset is anything inside a container).
  const ticks = $derived<'container' | 'file' | 'asset'>(
    entry?.viewer === 'container' ? 'container' : entry && entry.size == null ? 'asset' : 'file',
  );
  const inBasket = $derived(store.isTicked(path, ticks));

  function toggleBasket() {
    store.toggleTick({ path, kind: ticks, name });
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      store.toast('success', 'Copied', 1400);
    } catch {
      store.toast('error', 'Could not copy');
    }
  }

  function findDependency(guid: string) {
    store.searchFor(guid);
  }

  const score = $derived(entry?.score ?? 0);
</script>

<section class="viewer">
  <header>
    <div class="crumbs" title={path}>
      <span class="ico" style:color={categoryColor[cat]}><Icon name="file" size={18} /></span>
      <div class="path">
        {#each crumbs.slice(0, -1) as c, i}
          <span class="c">{c}</span><span class="sep">/</span>
        {/each}
        <span class="leaf">{name}</span>
      </div>
    </div>
    <div class="actions">
      <button class="btn sm ghost" onclick={() => copy(path)} title="Copy the path"><Icon name="copy" size={14} /></button>
      <button class="btn sm ghost" onclick={() => store.select(path, true)} title="Show in the tree"><Icon name="folder-open" size={14} /></button>
      <button class="btn sm" class:primary={!inBasket} onclick={toggleBasket}>
        <Icon name={inBasket ? 'check' : 'basket'} size={14} />
        {inBasket ? 'In basket' : 'Add to basket'}
      </button>
    </div>
  </header>

  {#if loading && !entry}
    <div class="empty"><span class="spin"><Icon name="refresh" size={22} /></span></div>
  {:else if error}
    <div class="empty err">
      <Icon name="alert" size={26} />
      <div class="t">Could not read this</div>
      <div class="s">{error}</div>
    </div>
  {:else if entry}
    <div class="chips">
      {#if entry.format}
        <span class="chip accent">{entry.format}{entry.version ? ` v${entry.version}` : ''}</span>
      {:else}
        <span class="chip">unknown format</span>
      {/if}
      {#if entry.kind && entry.kind !== entry.format}<span class="chip">{entry.kind}</span>{/if}
      {#if entry.size != null}<span class="chip">{formatSize(entry.size)}</span>{/if}
      {#if entry.format}
        <span class="chip {score >= 20 ? 'ok' : ''}" title="How sure the detection is">score {score}</span>
      {/if}
      {#if (entry.alternatives?.length ?? 0) > 0 && entry.alternatives![0].score === score}
        <span class="chip err" title={entry.alternatives!.map((a) => a.format).join(', ')}>ambiguous</span>
      {/if}
    </div>

    <div class="body">
      <div class="stage">
        {#if entry.viewer === 'image'}
          <ImageView {path} {entry} />
        {:else if entry.viewer === 'atlas'}
          <AtlasView {path} />
        {:else if entry.viewer === 'audio'}
          <AudioView {path} {entry} />
        {:else if entry.viewer === 'text'}
          <TextView {path} {entry} />
        {:else if entry.viewer === 'container'}
          <div class="note">
            <Icon name="package" size={28} />
            <div>
              <div class="t">A container</div>
              <div class="s">Expand it in the tree to browse its entries, or tick it to convert everything inside.</div>
            </div>
          </div>
        {:else}
          <div class="note">
            <Icon name="info" size={28} />
            <div>
              <div class="t">No preview for this format</div>
              <div class="s">
                {#if (entry.targets?.length ?? 0) > 0}It can still be converted — add it to the basket.{:else}Nothing here can convert it.{/if}
              </div>
            </div>
          </div>
          {#if entry.hex}<HexView {path} />{/if}
        {/if}
      </div>

      <aside class="details">
        {#if entry.bitmap}
          <section>
            <h4>Image</h4>
            <dl>
              <dt>Size</dt><dd>{entry.bitmap.width}×{entry.bitmap.height}{entry.bitmap.depth > 1 ? `×${entry.bitmap.depth}` : ''}</dd>
              <dt>Pixels</dt><dd>{entry.bitmap.pixelFormat}</dd>
              <dt>Mips</dt><dd>{entry.bitmap.mips}</dd>
              {#if entry.bitmap.layers > 1}<dt>Layers</dt><dd>{entry.bitmap.layers}</dd>{/if}
            </dl>
          </section>
        {/if}

        {#if entry.meta}
          <section>
            <h4>Asset</h4>
            <dl>
              {#if entry.meta.name}<dt>Name</dt><dd class="mono wrap">{entry.meta.name}</dd>{/if}
              {#if entry.meta.kind}<dt>Kind</dt><dd>{entry.meta.kind}</dd>{/if}
              {#if entry.meta.guid}
                <dt>GUID</dt>
                <dd class="mono">
                  <button class="link" onclick={() => copy(entry!.meta!.guid!)} title="Copy">{entry.meta.guid}</button>
                </dd>
              {/if}
            </dl>
          </section>
          {#if entry.meta.dependencies.length}
            <section>
              <h4>Depends on <span class="chip">{entry.meta.dependencies.length}</span></h4>
              <ul class="deps">
                {#each entry.meta.dependencies as dep}
                  <li>
                    <button class="link" onclick={() => findDependency(dep.guid)} title="Find among the opened paks">
                      <Icon name="link" size={12} />
                      <span class="mono">{dep.name ?? dep.guid}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
        {/if}

        {#if (entry.targets?.length ?? 0) > 0}
          <section>
            <h4>Can become</h4>
            <div class="targets">
              {#each entry.targets! as t}
                <span class="chip" title={t.label}>{t.type}</span>
              {/each}
            </div>
          </section>
        {/if}

        {#if (entry.alternatives?.length ?? 0) > 0}
          <section>
            <h4>Also looks like</h4>
            <div class="targets">
              {#each entry.alternatives! as a}
                <span class="chip" title="score {a.score}">{a.format}{a.version ? ` v${a.version}` : ''}</span>
              {/each}
            </div>
          </section>
        {/if}
      </aside>
    </div>
  {/if}
</section>

<style>
  .viewer {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 18px 22px 0;
    gap: 12px;
    height: 100%;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
  }
  .crumbs {
    display: flex;
    gap: 10px;
    align-items: center;
    min-width: 0;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    font-size: 15px;
  }
  .path > * {
    direction: ltr;
    unicode-bidi: embed;
  }
  .c,
  .sep {
    color: var(--faint);
  }
  .leaf {
    font-weight: 650;
    font-size: 17px;
  }
  .actions {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .chips {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 270px;
    gap: 18px;
    flex: 1;
    min-height: 0;
    padding-bottom: 18px;
  }
  .stage {
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-width: 0;
    min-height: 0;
    overflow: auto;
  }
  .details {
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-height: 0;
    padding-right: 2px;
  }
  .details section {
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 12px 14px;
  }
  h4 {
    margin: 0 0 8px;
    font-size: 11.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 14px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  dd.wrap {
    word-break: break-all;
  }
  .targets {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .deps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
    max-height: 220px;
    overflow: auto;
  }
  .link {
    border: 0;
    background: transparent;
    padding: 2px 0;
    color: var(--text-2);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    text-align: left;
    word-break: break-all;
  }
  .link:hover {
    color: var(--accent);
  }
  .empty {
    flex: 1;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 8px;
    color: var(--muted);
  }
  .empty.err {
    color: var(--err);
  }
  .t {
    font-weight: 650;
    color: var(--text);
    font-size: 15px;
  }
  .s {
    color: var(--muted);
    max-width: 52ch;
  }
  .note {
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 22px;
    background: var(--bg-2);
    border: 1px dashed var(--line-2);
    border-radius: 12px;
    color: var(--muted);
  }
  @media (max-width: 1100px) {
    .body {
      grid-template-columns: 1fr;
      overflow: auto;
    }
    .details {
      overflow: visible;
    }
  }
</style>
