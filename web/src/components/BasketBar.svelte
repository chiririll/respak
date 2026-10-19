<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { categoryColor, categoryOf } from '../lib/util';

  const open = $derived(store.panel === 'basket');
</script>

{#if store.basket.length > 0}
  <div class="bar" role="region" aria-label="Basket">
    {#if open}
      <div class="list">
        <div class="lh">
          <span>Basket</span>
          <button class="btn sm ghost" onclick={() => store.clearBasket()}><Icon name="trash" size={13} /> Clear</button>
        </div>
        <ul>
          {#each store.basket as t (t.key)}
            {@const cat = categoryOf(t.kind === 'group' ? (t.ext ?? '') : t.name, t.kind)}
            <li>
              <span style:color={categoryColor[cat]}>
                <Icon name={t.kind === 'dir' || t.kind === 'folder' ? 'folder' : t.kind === 'container' ? 'package' : 'file'} size={14} />
              </span>
              <button class="p tail" title={t.path} onclick={() => t.kind !== 'dir' && t.kind !== 'folder' && store.select(t.path, true)}>
                <bdi dir="ltr">{t.kind === 'group' ? `all ${t.ext} · ${t.path}` : t.path || '/'}</bdi>
              </button>
              {#if t.kind === 'dir' || t.kind === 'folder' || t.kind === 'container'}<span class="chip">everything inside</span>{/if}
              <button class="btn icon sm ghost" onclick={() => store.untick(t.key)} aria-label="Remove"><Icon name="x" size={13} /></button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    <div class="strip">
      <button class="count" onclick={() => (store.panel = open ? 'none' : 'basket')}>
        <Icon name="basket" size={16} />
        <b>{store.basket.length}</b> selected
        <span class="up" class:flip={open}><Icon name="chevron" size={13} /></span>
      </button>
      <button class="btn primary" onclick={() => (store.convertOpen = true)}>
        Convert <Icon name="arrow-right" size={15} />
      </button>
    </div>
  </div>
{/if}

<style>
  .bar {
    position: fixed;
    left: 50%;
    bottom: 22px;
    transform: translateX(-50%);
    z-index: 30;
    display: grid;
    gap: 8px;
    width: min(560px, calc(100vw - 32px));
  }
  .strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 8px 8px 16px;
    background: color-mix(in srgb, var(--bg-3) 92%, transparent);
    backdrop-filter: blur(14px);
    border: 1px solid var(--line-2);
    border-radius: 14px;
    box-shadow: var(--shadow);
  }
  .count {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    border: 0;
    background: transparent;
    color: var(--text-2);
    padding: 4px 0;
  }
  .count b {
    color: var(--text);
  }
  .up {
    transform: rotate(-90deg);
    transition: transform 0.15s;
    display: grid;
  }
  .up.flip {
    transform: rotate(90deg);
  }
  .list {
    background: color-mix(in srgb, var(--bg-3) 94%, transparent);
    backdrop-filter: blur(14px);
    border: 1px solid var(--line-2);
    border-radius: 14px;
    box-shadow: var(--shadow);
    max-height: 320px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .lh {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 10px 4px 14px;
    font-weight: 650;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 4px 6px 8px;
    overflow: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 4px 2px 8px;
    border-radius: 8px;
  }
  li:hover {
    background: var(--hover);
  }
  .p {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    text-align: left;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    padding: 4px 0;
  }
</style>
