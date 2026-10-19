<script lang="ts">
  import { api, type JobView, type OutputView } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import { errorText, formatTime } from '../lib/util';

  let open = $state<Record<string, boolean>>({});
  const jobs = $derived([...store.jobs].sort((a, b) => b.id - a.id));
  const isImage = (f: string) => /\.(png|jpe?g|webp|gif)$/i.test(f);

  async function stop(job: JobView) {
    try {
      await api.cancelJob(job.id);
      if (job.state !== 'running') store.jobs = store.jobs.filter((j) => j.id !== job.id);
    } catch (e) {
      store.toast('error', errorText(e));
    }
  }

  function stats(job: JobView) {
    let ok = 0;
    let failed = 0;
    let files = 0;
    for (const r of job.rows) {
      if (r.error) failed++;
      for (const o of r.outputs) {
        if (o.ok) {
          ok++;
          files += o.files.length;
        } else failed++;
      }
    }
    return { ok, failed, files };
  }

  const key = (job: JobView, i: number) => `${job.id}:${i}`;
  function failedOutputs(outputs: OutputView[]) {
    return outputs.filter((o) => !o.ok);
  }
</script>

<aside class="panel" aria-label="Jobs">
  <header>
    <h2>Jobs</h2>
    <button class="btn icon sm ghost" onclick={() => (store.panel = 'none')} aria-label="Close"><Icon name="x" size={16} /></button>
  </header>

  <div class="scroll">
    {#each jobs as job (job.id)}
      {@const s = stats(job)}
      <article class="job">
        <div class="jh">
          <div class="title">
            <b>Job #{job.id}</b>
            <span class="muted">{formatTime(job.started)}</span>
          </div>
          {#if job.state === 'running'}
            <span class="chip accent"><span class="spin"><Icon name="refresh" size={11} /></span> running</span>
          {:else if job.state === 'cancelled'}
            <span class="chip">cancelled</span>
          {:else if s.failed}
            <span class="chip err">{s.failed} failed</span>
          {:else}
            <span class="chip ok"><Icon name="check" size={11} /> done</span>
          {/if}
          <button class="btn sm ghost" class:danger={job.state === 'running'} onclick={() => stop(job)}>
            {job.state === 'running' ? 'Cancel' : 'Dismiss'}
          </button>
        </div>

        <div class="bar"><div class="fill" class:animate={job.state === 'running'} style:width="{job.total ? (job.done / job.total) * 100 : 0}%"></div></div>
        <div class="sub muted">
          {job.done}/{job.total} items · {s.files} files written{s.failed ? ` · ${s.failed} failed` : ''}
        </div>

        <ul class="rows">
          {#each job.rows as row, i}
            {@const k = key(job, i)}
            {@const bad = failedOutputs(row.outputs)}
            <li>
              <button class="rh" onclick={() => (open[k] = !open[k])}>
                <span class="st {row.status}">
                  {#if row.status === 'running'}<span class="spin"><Icon name="refresh" size={12} /></span>
                  {:else if row.error || bad.length}<Icon name="alert" size={12} />
                  {:else if row.status === 'finished'}<Icon name="check" size={12} />
                  {:else}<span class="dot"></span>{/if}
                </span>
                <span class="from tail" title={row.from}><bdi dir="ltr">{row.from}</bdi></span>
                <span class="chip">{row.target}</span>
                <span class="n muted">{row.outputs.length > 1 ? row.outputs.length : ''}</span>
              </button>

              {#if row.error}
                <div class="error"><b>{row.error.kind}</b> {row.error.message}</div>
              {/if}
              {#each bad.slice(0, open[k] ? 200 : 3) as o}
                <div class="error"><span class="mono">{o.from}</span><br /><b>{o.error?.kind}</b> {o.error?.message}</div>
              {/each}
              {#if !open[k] && bad.length > 3}<div class="muted more">… {bad.length - 3} more failures</div>{/if}

              {#if open[k] || row.outputs.length <= 3}
                {#each row.outputs.filter((o) => o.ok).slice(0, open[k] ? 500 : 3) as o}
                  {#each o.files as f}
                    <div class="file">
                      {#if isImage(f)}
                        <a href={api.outputUrl(f)} target="_blank" rel="noreferrer" class="thumb checker"><img src={api.outputUrl(f)} alt="" loading="lazy" /></a>
                      {:else}
                        <span class="fi"><Icon name="file" size={14} /></span>
                      {/if}
                      <a class="fn tail mono" href={api.outputUrl(f)} target="_blank" rel="noreferrer" title={f}><bdi dir="ltr">{f}</bdi></a>
                      <a class="btn icon sm ghost" href={api.outputUrl(f, true)} download title="Download"><Icon name="download" size={14} /></a>
                    </div>
                  {/each}
                {/each}
              {:else}
                <button class="expand" onclick={() => (open[k] = true)}>
                  Show {row.outputs.filter((o) => o.ok).reduce((n, o) => n + o.files.length, 0)} written files
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      </article>
    {:else}
      <div class="none">
        <Icon name="jobs" size={28} />
        <div>No jobs yet.</div>
        <div class="muted">Tick files in the tree and press Convert.</div>
      </div>
    {/each}

    {#if store.log.length}
      <details class="log">
        <summary>Activity ({store.log.length})</summary>
        <div class="lines mono">
          {#each [...store.log].reverse().slice(0, 100) as l (l.id)}
            <div class:warn={l.level === 'warn'}>{l.message}</div>
          {/each}
        </div>
      </details>
    {/if}
  </div>
</aside>

<style>
  .panel {
    position: fixed;
    top: 56px;
    right: 0;
    bottom: 0;
    width: min(460px, 100vw);
    z-index: 40;
    background: var(--bg-2);
    border-left: 1px solid var(--line-2);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 14px 12px 18px;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .scroll {
    overflow: auto;
    padding: 14px;
    display: grid;
    gap: 14px;
    align-content: start;
    flex: 1;
  }
  .job {
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 12px 14px;
  }
  .jh {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .title {
    flex: 1;
    display: grid;
  }
  .bar {
    height: 5px;
    background: var(--bg-4);
    border-radius: 99px;
    overflow: hidden;
    margin: 10px 0 6px;
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    border-radius: 99px;
    transition: width 0.25s;
  }
  .fill.animate {
    background-size: 200% 100%;
    animation: slide 1.4s linear infinite;
    background-image: linear-gradient(90deg, var(--accent), var(--accent-2), var(--accent));
  }
  @keyframes slide {
    to {
      background-position: -200% 0;
    }
  }
  .sub {
    font-size: 12.5px;
  }
  .rows {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .rows > li {
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 9px;
    padding: 6px 8px;
  }
  .rh {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    border: 0;
    background: transparent;
    text-align: left;
    padding: 2px 0;
  }
  .from {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    font-size: 13px;
  }
  .st {
    display: grid;
    place-items: center;
    width: 16px;
    color: var(--muted);
  }
  .st.finished {
    color: var(--ok);
  }
  .st.running {
    color: var(--accent);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .error {
    margin: 6px 0 2px 24px;
    padding: 6px 8px;
    background: color-mix(in srgb, var(--err) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--err) 30%, transparent);
    color: var(--err);
    border-radius: 7px;
    font-size: 12.5px;
    word-break: break-word;
  }
  .more,
  .expand {
    margin-left: 24px;
    font-size: 12.5px;
  }
  .expand {
    border: 0;
    background: transparent;
    color: var(--accent);
    padding: 2px 0;
  }
  .file {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 4px 0 0 24px;
  }
  .thumb {
    width: 34px;
    height: 34px;
    flex: none;
    border-radius: 6px;
    overflow: hidden;
    border: 1px solid var(--line-2);
    display: block;
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .fi {
    width: 34px;
    height: 34px;
    flex: none;
    display: grid;
    place-items: center;
    color: var(--faint);
  }
  .fn {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
    text-decoration: none;
    direction: rtl;
    text-align: left;
  }
  .fn:hover {
    color: var(--accent);
  }
  a.btn {
    text-decoration: none;
  }
  .none {
    display: grid;
    justify-items: center;
    gap: 6px;
    padding: 50px 0;
    color: var(--faint);
  }
  .log {
    color: var(--muted);
  }
  .log summary {
    cursor: pointer;
    font-weight: 600;
  }
  .lines {
    margin-top: 8px;
    max-height: 200px;
    overflow: auto;
    display: grid;
    gap: 2px;
    font-size: 12px;
  }
  .lines .warn {
    color: var(--warn);
  }
</style>
