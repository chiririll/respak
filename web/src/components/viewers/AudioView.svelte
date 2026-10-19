<script lang="ts">
  import { api, blobUrl, type EntryView } from '../../lib/api';
  import Icon from '../../lib/Icon.svelte';
  import { errorText } from '../../lib/util';

  let { path, entry }: { path: string; entry: EntryView } = $props();

  const tracks = $derived(entry.audio ?? [{ channels: 0, sampleRate: 0 }]);
  let track = $state(0);
  let url = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let audio: HTMLAudioElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let playing = $state(false);
  let time = $state(0);
  let duration = $state(0);
  let peaks: number[] = [];

  $effect(() => {
    path;
    track = 0;
  });

  $effect(() => {
    const abort = new AbortController();
    loading = true;
    error = null;
    peaks = [];
    playing = false;
    (async () => {
      const res = await fetch(api.audioUrl(path, track), { signal: abort.signal });
      if (!res.ok) {
        const data = await res.json().catch(() => null);
        throw new Error(data?.error?.message ?? res.statusText);
      }
      const blob = await res.blob();
      peaks = await waveform(await blob.arrayBuffer());
      const next = URL.createObjectURL(blob);
      const old = url;
      url = next;
      if (old) URL.revokeObjectURL(old);
    })()
      .catch((e) => {
        if (!abort.signal.aborted) error = errorText(e);
      })
      .finally(() => {
        if (!abort.signal.aborted) loading = false;
      });
    return () => abort.abort();
  });

  async function waveform(data: ArrayBuffer): Promise<number[]> {
    try {
      const ctx = new OfflineAudioContext(1, 1, 44100);
      const buffer = await ctx.decodeAudioData(data);
      const channel = buffer.getChannelData(0);
      const bars = 240;
      const step = Math.max(1, Math.floor(channel.length / bars));
      const out: number[] = [];
      for (let i = 0; i < bars; i++) {
        let max = 0;
        for (let j = i * step; j < Math.min(channel.length, (i + 1) * step); j += Math.max(1, step >> 6)) {
          max = Math.max(max, Math.abs(channel[j]));
        }
        out.push(max);
      }
      return out;
    } catch {
      return [];
    }
  }

  function draw() {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    const g = canvas.getContext('2d')!;
    g.scale(dpr, dpr);
    g.clearRect(0, 0, w, h);
    const style = getComputedStyle(canvas);
    const accent = style.getPropertyValue('--accent').trim() || '#ff6a3d';
    const dim = style.getPropertyValue('--line-2').trim() || '#444';
    const n = peaks.length || 60;
    const bw = w / n;
    const progress = duration ? time / duration : 0;
    for (let i = 0; i < n; i++) {
      const p = peaks.length ? Math.max(0.03, peaks[i]) : 0.04;
      const bh = p * (h - 8);
      g.fillStyle = i / n < progress ? accent : dim;
      g.beginPath();
      g.roundRect(i * bw + 1, (h - bh) / 2, Math.max(1, bw - 2), bh, 2);
      g.fill();
    }
  }

  $effect(() => {
    peaks;
    time;
    duration;
    draw();
  });

  function toggle() {
    if (!audio) return;
    if (audio.paused) void audio.play();
    else audio.pause();
  }

  function seek(e: MouseEvent) {
    if (!audio || !canvas || !duration) return;
    const r = canvas.getBoundingClientRect();
    audio.currentTime = ((e.clientX - r.left) / r.width) * duration;
  }

  const mmss = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
</script>

<div class="card">
  <div class="top">
    <button class="play" onclick={toggle} disabled={!url} aria-label={playing ? 'Pause' : 'Play'}>
      <Icon name={playing ? 'pause' : 'play'} size={22} />
    </button>
    <div class="wave">
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <canvas bind:this={canvas} onclick={seek} aria-label="Waveform"></canvas>
      <div class="times mono"><span>{mmss(time)}</span><span>{duration ? mmss(duration) : '--:--'}</span></div>
    </div>
  </div>

  {#if loading}
    <div class="state"><span class="spin"><Icon name="refresh" size={16} /></span> Decoding…</div>
  {:else if error}
    <div class="state err"><Icon name="alert" size={16} /> {error}</div>
  {/if}

  {#if url}
    <audio
      bind:this={audio}
      src={url}
      onplay={() => (playing = true)}
      onpause={() => (playing = false)}
      onended={() => (playing = false)}
      ontimeupdate={() => (time = audio!.currentTime)}
      onloadedmetadata={() => (duration = audio!.duration)}
    ></audio>
  {/if}

  <div class="info">
    {#if tracks.length > 1}
      <div class="tracks">
        <span class="lbl">Track</span>
        {#each tracks as t, i}
          <button class:on={track === i} onclick={() => (track = i)}>{i}</button>
        {/each}
      </div>
    {/if}
    {#if tracks[track]?.sampleRate}
      <span class="chip">{tracks[track].channels === 1 ? 'mono' : tracks[track].channels === 2 ? 'stereo' : `${tracks[track].channels} ch`}</span>
      <span class="chip">{(tracks[track].sampleRate / 1000).toFixed(tracks[track].sampleRate % 1000 ? 1 : 0)} kHz</span>
    {/if}
    {#if url}
      <a class="btn sm" href={url} download={path.split('/').pop() + '.wav'}><Icon name="download" size={14} /> WAV</a>
    {/if}
  </div>
</div>

<style>
  .card {
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 20px;
    display: grid;
    gap: 16px;
    max-width: 820px;
  }
  .top {
    display: flex;
    gap: 18px;
    align-items: center;
  }
  .play {
    flex: none;
    width: 58px;
    height: 58px;
    border-radius: 50%;
    border: 0;
    color: #fff;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    display: grid;
    place-items: center;
    box-shadow: 0 8px 22px -8px var(--accent);
  }
  .play:disabled {
    opacity: 0.4;
  }
  .wave {
    flex: 1;
    min-width: 0;
  }
  canvas {
    width: 100%;
    height: 72px;
    display: block;
    cursor: pointer;
  }
  .times {
    display: flex;
    justify-content: space-between;
    color: var(--muted);
    margin-top: 4px;
  }
  .state {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
  }
  .state.err {
    color: var(--err);
  }
  .info {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .tracks {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: 9px;
    padding: 2px;
  }
  .lbl {
    color: var(--muted);
    font-size: 12px;
    padding: 0 8px;
    font-weight: 600;
  }
  .tracks button {
    border: 0;
    background: transparent;
    min-width: 28px;
    height: 26px;
    border-radius: 7px;
    color: var(--muted);
    font-weight: 600;
  }
  .tracks button.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  a.btn {
    text-decoration: none;
  }
</style>
