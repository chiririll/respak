// Global state of the page: what is connected, selected, ticked and running.

import {
  api,
  ApiError,
  type BasketItem,
  type ConfigState,
  type JobView,
  type LoginEvent,
  type NodeKind,
  type SessionInfo,
  type UiEvent,
} from './api';

export type Page = 'loading' | 'settings' | 'explorer';
export type Theme = 'dark' | 'light';

export interface Toast {
  id: number;
  kind: 'info' | 'error' | 'success';
  text: string;
}

export interface LogLine {
  id: number;
  level: 'info' | 'warn';
  message: string;
  at: number;
}

/** One ticked row of the tree. */
export interface Ticked {
  key: string;
  path: string;
  kind: NodeKind;
  /** For a group: the kind; for a folder, container or directory: an optional filter. */
  ext?: string;
  name: string;
}

function loadTheme(): Theme {
  try {
    const saved = localStorage.getItem('respak.theme');
    if (saved === 'light' || saved === 'dark') return saved;
  } catch {
    /* storage may be blocked */
  }
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

class Store {
  page = $state<Page>('loading');
  theme = $state<Theme>(loadTheme());
  configState = $state<ConfigState | null>(null);
  session = $state<SessionInfo | null>(null);
  steamStats = $state<{ chunksDownloaded: number; bytesDownloaded: number; cacheHits: number } | null>(null);
  selected = $state<string | null>(null);
  /** Increments to ask the tree to scroll to and open `selected`. */
  revealTick = $state(0);
  basket = $state<Ticked[]>([]);
  jobs = $state<JobView[]>([]);
  log = $state<LogLine[]>([]);
  toasts = $state<Toast[]>([]);
  progress = $state<{ task: string; done: number; total: number } | null>(null);
  panel = $state<'none' | 'jobs' | 'basket'>('none');
  convertOpen = $state(false);
  live = $state(false);
  /** The last word of a Steam login in progress. */
  steamLogin = $state<LoginEvent | null>(null);
  /** Increments when a login finishes, so the settings reload the account. */
  steamLoginDone = $state(0);
  /** A search the sidebar should run (a dependency clicked in the viewer). */
  searchRequest = $state<{ q: string; tick: number }>({ q: '', tick: 0 });

  private toastId = 0;
  private logId = 0;
  private source: EventSource | null = null;

  get running(): number {
    return this.jobs.filter((j) => j.state === 'running').length;
  }

  setTheme(theme: Theme) {
    this.theme = theme;
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem('respak.theme', theme);
    } catch {
      /* ignore */
    }
  }

  toast(kind: Toast['kind'], text: string, ms = 4500) {
    const id = ++this.toastId;
    this.toasts.push({ id, kind, text });
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), ms);
  }

  fail(error: unknown) {
    this.toast('error', error instanceof ApiError || error instanceof Error ? error.message : String(error), 7000);
  }

  async init() {
    document.documentElement.dataset.theme = this.theme;
    this.listen();
    try {
      this.configState = await api.config();
      const state = await api.session();
      this.applySession(state.connected ? state.session! : null);
      this.jobs = await api.jobs();
      this.page = state.connected ? 'explorer' : 'settings';
    } catch (e) {
      this.page = 'settings';
      this.fail(e);
    }
  }

  applySession(session: SessionInfo | null) {
    this.session = session;
    if (!session) {
      this.selected = null;
      this.steamStats = null;
    }
  }

  async refreshConfig() {
    this.configState = await api.config();
  }

  async connect() {
    const state = await api.connect();
    this.applySession(state.session ?? null);
    this.selected = null;
    this.page = 'explorer';
  }

  async disconnect() {
    await api.disconnect();
    this.applySession(null);
    this.page = 'settings';
  }

  searchFor(q: string) {
    this.searchRequest = { q, tick: this.searchRequest.tick + 1 };
  }

  select(path: string | null, reveal = false) {
    this.selected = path;
    if (reveal && path) this.revealTick++;
  }

  // --------------------------------------------------------------- basket

  tickKey(path: string, kind: NodeKind, ext?: string) {
    return `${kind}|${path}|${ext ?? ''}`;
  }

  isTicked(path: string, kind: NodeKind, ext?: string) {
    const key = this.tickKey(path, kind, ext);
    return this.basket.some((t) => t.key === key);
  }

  toggleTick(item: { path: string; kind: NodeKind; ext?: string; name: string }) {
    const key = this.tickKey(item.path, item.kind, item.ext);
    if (this.basket.some((t) => t.key === key)) {
      this.basket = this.basket.filter((t) => t.key !== key);
    } else {
      this.basket = [...this.basket, { ...item, key }];
    }
  }

  untick(key: string) {
    this.basket = this.basket.filter((t) => t.key !== key);
  }

  clearBasket() {
    this.basket = [];
  }

  // ----------------------------------------------------------------- events

  private listen() {
    if (this.source) return;
    const source = new EventSource('/api/events');
    this.source = source;
    source.onopen = () => (this.live = true);
    source.onerror = () => (this.live = false);
    source.onmessage = (message) => {
      let event: UiEvent;
      try {
        event = JSON.parse(message.data);
      } catch {
        return;
      }
      this.onEvent(event);
    };
  }

  private onEvent(event: UiEvent) {
    switch (event.type) {
      case 'job': {
        const { type: _type, ...job } = event;
        const i = this.jobs.findIndex((j) => j.id === job.id);
        const before = i >= 0 ? this.jobs[i].state : null;
        if (i >= 0) this.jobs[i] = job;
        else this.jobs.push(job);
        if (before === 'running' && job.state !== 'running') {
          const failed = job.rows.reduce((n, r) => n + r.outputs.filter((o) => !o.ok).length + (r.error ? 1 : 0), 0);
          const written = job.rows.reduce((n, r) => n + r.outputs.reduce((m, o) => m + o.files.length, 0), 0);
          if (job.state === 'cancelled') this.toast('info', `Job #${job.id} cancelled`);
          else if (failed) this.toast('error', `Job #${job.id}: ${failed} failed, ${written} files written`, 7000);
          else this.toast('success', `Job #${job.id} finished: ${written} files written`);
        }
        break;
      }
      case 'log':
        this.log.push({ id: ++this.logId, level: event.level, message: event.message, at: Date.now() });
        if (this.log.length > 300) this.log.splice(0, this.log.length - 300);
        break;
      case 'progress':
        this.progress = event.done >= event.total ? null : { task: event.task, done: event.done, total: event.total };
        break;
      case 'steam':
        void api.session().then((s) => (this.steamStats = s.steam ?? null)).catch(() => {});
        break;
      case 'steamLogin': {
        const { type: _type, ...login } = event;
        this.steamLogin = login as LoginEvent;
        if (login.state === 'done') {
          this.steamLoginDone++;
          this.toast('success', `Signed in to Steam as ${login.accountName}`);
        }
        break;
      }
      case 'session':
        if (!event.connected && this.session) {
          // The server dropped the session (it restarted, or another page disconnected).
          this.applySession(null);
          if (this.page === 'explorer') {
            this.page = 'settings';
            this.toast('info', 'The session ended: connect again to continue');
          }
        }
        break;
    }
  }
}

export const store = new Store();

/** The requests a ticked row stands for, with the target chosen in the dialog. */
export function basketRequests(
  basket: Ticked[],
  to: BasketItem['to'],
  filter: Record<string, string>,
): BasketItem[] {
  return basket.map((t) => {
    const glob = (ext?: string) => (ext ? `**/*.${ext}` : '**/*');
    switch (t.kind) {
      case 'file':
      case 'asset':
        return { from: t.path, to };
      case 'group':
        return { from: `${t.path}/**/*.${t.ext}`, to, skip_unsupported: true };
      default: {
        const ext = filter[t.key]?.trim().replace(/^\./, '') || t.ext;
        return { from: `${t.path ? t.path + '/' : ''}${glob(ext)}`, to, skip_unsupported: true };
      }
    }
  });
}
