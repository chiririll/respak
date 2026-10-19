<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each store.toasts as t (t.id)}
    <div class="toast {t.kind}">
      <Icon name={t.kind === 'error' ? 'alert' : t.kind === 'success' ? 'check' : 'info'} size={16} />
      <span>{t.text}</span>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    top: 66px;
    right: 16px;
    z-index: 100;
    display: grid;
    gap: 8px;
    pointer-events: none;
    max-width: min(420px, calc(100vw - 32px));
  }
  .toast {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 14px;
    background: var(--bg-3);
    border: 1px solid var(--line-2);
    border-radius: 11px;
    box-shadow: var(--shadow);
    animation: in 0.18s ease-out;
    word-break: break-word;
  }
  .toast.error {
    border-color: color-mix(in srgb, var(--err) 50%, transparent);
    color: var(--err);
  }
  .toast.success :global(.icon) {
    color: var(--ok);
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
  }
</style>
