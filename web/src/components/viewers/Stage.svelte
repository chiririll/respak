<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '../../lib/Icon.svelte';

  // A pan-and-zoom stage for one picture on a checkerboard. `overlay` is drawn in image pixels.
  let {
    url,
    loading = false,
    error = null,
    onsize,
    overlay,
  }: {
    url: string | null;
    loading?: boolean;
    error?: string | null;
    onsize?: (w: number, h: number) => void;
    overlay?: Snippet<[{ scale: number; w: number; h: number }]>;
  } = $props();

  let box: HTMLDivElement;
  let w = $state(0);
  let h = $state(0);
  let scale = $state(1);
  let x = $state(0);
  let y = $state(0);
  let fitted = $state(true);
  let drag: { px: number; py: number; x: number; y: number } | null = null;

  function fit() {
    if (!box || !w || !h) return;
    const s = Math.min((box.clientWidth - 32) / w, (box.clientHeight - 32) / h, 8);
    scale = s > 0 ? s : 1;
    x = (box.clientWidth - w * scale) / 2;
    y = (box.clientHeight - h * scale) / 2;
    fitted = true;
  }

  function actual() {
    zoomAt(1, box.clientWidth / 2, box.clientHeight / 2, true);
  }

  function zoomAt(next: number, cx: number, cy: number, absolute = false) {
    const target = Math.min(32, Math.max(0.05, absolute ? next : scale * next));
    const k = target / scale;
    x = cx - (cx - x) * k;
    y = cy - (cy - y) * k;
    scale = target;
    fitted = false;
  }

  function onload(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    w = img.naturalWidth;
    h = img.naturalHeight;
    onsize?.(w, h);
    fit();
  }

  function wheel(e: WheelEvent) {
    e.preventDefault();
    const r = box.getBoundingClientRect();
    zoomAt(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top);
  }

  function down(e: PointerEvent) {
    drag = { px: e.clientX, py: e.clientY, x, y };
    box.setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    x = drag.x + e.clientX - drag.px;
    y = drag.y + e.clientY - drag.py;
    fitted = false;
  }
  function up() {
    drag = null;
  }

  $effect(() => {
    const observer = new ResizeObserver(() => fitted && fit());
    observer.observe(box);
    return () => observer.disconnect();
  });
</script>

<div
  class="stage checker"
  bind:this={box}
  onwheel={wheel}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  ondblclick={() => (fitted ? actual() : fit())}
  role="img"
  aria-label="Preview"
>
  {#if url}
    <div class="canvas" style:transform="translate({x}px, {y}px) scale({scale})" style:width="{w}px" style:height="{h}px">
      <img src={url} alt="" draggable="false" onload={onload} style:image-rendering={scale > 2 ? 'pixelated' : 'auto'} />
      {#if overlay}{@render overlay({ scale, w, h })}{/if}
    </div>
  {/if}

  {#if loading}
    <div class="center"><span class="spin"><Icon name="refresh" size={22} /></span></div>
  {/if}
  {#if error}
    <div class="center error">
      <Icon name="alert" size={22} />
      <div>{error}</div>
    </div>
  {/if}

  <div class="tools" role="toolbar" tabindex="-1" aria-label="Zoom" onpointerdown={(e) => e.stopPropagation()} onpointermove={(e) => e.stopPropagation()}>
    <button class="btn icon sm ghost" title="Zoom out" onclick={() => zoomAt(0.8, box.clientWidth / 2, box.clientHeight / 2)}><Icon name="zoom-out" size={15} /></button>
    <span class="zoom mono">{Math.round(scale * 100)}%</span>
    <button class="btn icon sm ghost" title="Zoom in" onclick={() => zoomAt(1.25, box.clientWidth / 2, box.clientHeight / 2)}><Icon name="zoom-in" size={15} /></button>
    <button class="btn icon sm ghost" title="Fit to window (double-click)" onclick={fit}><Icon name="fit" size={15} /></button>
    <button class="btn sm ghost" title="Actual size" onclick={actual}>1:1</button>
  </div>
</div>

<style>
  .stage {
    position: relative;
    flex: 1;
    min-height: 240px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 12px;
    cursor: grab;
    touch-action: none;
  }
  .stage:active {
    cursor: grabbing;
  }
  .canvas {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.08);
  }
  img {
    display: block;
    width: 100%;
    height: 100%;
    user-select: none;
  }
  .center {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 8px;
    color: var(--muted);
    text-align: center;
    padding: 24px;
    pointer-events: none;
  }
  .center.error {
    color: var(--err);
    background: color-mix(in srgb, var(--bg-2) 85%, transparent);
  }
  .tools {
    position: absolute;
    right: 10px;
    bottom: 10px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    background: color-mix(in srgb, var(--bg-2) 88%, transparent);
    backdrop-filter: blur(8px);
    border: 1px solid var(--line-2);
    border-radius: 10px;
    cursor: default;
  }
  .zoom {
    min-width: 46px;
    text-align: center;
    color: var(--text-2);
  }
</style>
