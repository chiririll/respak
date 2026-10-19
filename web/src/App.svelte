<script lang="ts">
  import { onMount } from 'svelte';
  import BasketBar from './components/BasketBar.svelte';
  import ConvertDialog from './components/ConvertDialog.svelte';
  import JobsPanel from './components/JobsPanel.svelte';
  import Settings from './components/Settings.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Toasts from './components/Toasts.svelte';
  import TopBar from './components/TopBar.svelte';
  import Viewer from './components/Viewer.svelte';
  import Icon from './lib/Icon.svelte';
  import { store } from './lib/store.svelte';

  onMount(() => {
    void store.init();
  });

  let width = $state(Number(safeGet('respak.sidebar')) || 380);
  let dragging = false;

  function safeGet(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  function startDrag(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function drag(e: PointerEvent) {
    if (dragging) width = Math.min(720, Math.max(260, e.clientX));
  }
  function endDrag() {
    if (!dragging) return;
    dragging = false;
    try {
      localStorage.setItem('respak.sidebar', String(width));
    } catch {
      /* ignore */
    }
  }
</script>

<div class="app">
  <TopBar />

  {#if store.page === 'loading'}
    <div class="boot"><span class="spin"><Icon name="refresh" size={24} /></span></div>
  {:else if store.page === 'settings'}
    <main class="single"><Settings /></main>
  {:else}
    <main class="explorer" style:grid-template-columns="{width}px 6px minmax(0, 1fr)">
      <Sidebar />
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="grip" onpointerdown={startDrag} onpointermove={drag} onpointerup={endDrag} onpointercancel={endDrag}></div>
      <div class="content">
        {#if store.selected}
          {#key store.selected}
            <Viewer path={store.selected} />
          {/key}
        {:else}
          <div class="welcome">
            <div class="art"><Icon name="package" size={44} /></div>
            <h2>Pick something to look at</h2>
            <p>Open a folder or a pak in the tree, or search for a name. Pictures, atlases and sounds preview right here.</p>
            <ul>
              <li><Icon name="check" size={14} /> Tick the boxes to collect files in the basket</li>
              <li><Icon name="check" size={14} /> A ticked folder or pak stands for everything inside it</li>
              <li><Icon name="check" size={14} /> Converted files appear under <b>Jobs</b>, ready to download</li>
            </ul>
          </div>
        {/if}
      </div>
    </main>
  {/if}

  {#if store.panel === 'jobs'}<JobsPanel />{/if}
  {#if store.page === 'explorer'}<BasketBar />{/if}
  {#if store.convertOpen}<ConvertDialog onclose={() => (store.convertOpen = false)} />{/if}
  <Toasts />
</div>

<style>
  .app {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  main {
    flex: 1;
    min-height: 0;
  }
  .explorer {
    display: grid;
  }
  .grip {
    cursor: col-resize;
    background: transparent;
    position: relative;
    z-index: 5;
    margin-left: -3px;
    width: 6px;
  }
  .grip:hover,
  .grip:active {
    background: var(--accent-soft);
  }
  .content {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background:
      radial-gradient(800px 300px at 100% -10%, color-mix(in srgb, var(--accent) 6%, transparent), transparent 70%),
      var(--bg);
  }
  .boot {
    flex: 1;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .welcome {
    height: 100%;
    display: grid;
    place-content: center;
    justify-items: center;
    text-align: center;
    gap: 6px;
    padding: 40px;
    color: var(--muted);
  }
  .art {
    width: 84px;
    height: 84px;
    border-radius: 24px;
    display: grid;
    place-items: center;
    color: var(--accent);
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    margin-bottom: 10px;
  }
  h2 {
    margin: 0;
    color: var(--text);
    font-size: 21px;
  }
  p {
    margin: 0;
    max-width: 46ch;
  }
  ul {
    list-style: none;
    margin: 16px 0 0;
    padding: 0;
    display: grid;
    gap: 8px;
    text-align: left;
  }
  li {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  li :global(.icon) {
    color: var(--ok);
  }
</style>
