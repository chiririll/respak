// The server's API (docs/design/webui.md section 5), typed.

export class ApiError extends Error {
  constructor(
    public status: number,
    public kind: string,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method,
    // Every request says it is JSON: the server refuses state changes that do not.
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let kind = 'Error';
    let message = `${res.status} ${res.statusText}`;
    try {
      const data = await res.json();
      kind = data.error?.kind ?? kind;
      message = data.error?.message ?? message;
    } catch {
      /* not JSON */
    }
    throw new ApiError(res.status, kind, message);
  }
  return (await res.json()) as T;
}

const qs = (params: Record<string, string | number | boolean | undefined | null>) => {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '' && v !== false) q.set(k, String(v));
  }
  const s = q.toString();
  return s ? `?${s}` : '';
};

// ---------------------------------------------------------------- types

export interface SteamConfig {
  app: number;
  depots: (number | string)[];
  branch?: string;
  cache_size?: string;
  verbose?: boolean;
}
export interface Config {
  root?: string;
  out?: string;
  cache?: string;
  report?: string;
  search?: string[];
  no_patches?: boolean;
  respawn?: { rig?: string; studio?: string };
  steam?: SteamConfig;
}
export interface ConfigState {
  file: string;
  exists: boolean;
  cwd: string;
  steamAvailable: boolean;
  config: Config;
}
export interface SessionInfo {
  source: 'root' | 'steam';
  root: string | null;
  out: string;
  steam: { app: number; branch: string; depots: string[] } | null;
  containers: string[];
  formats: { name: string; label: string }[];
}
export interface SessionState {
  connected: boolean;
  session?: SessionInfo;
  steam?: { chunksDownloaded: number; bytesDownloaded: number; cacheHits: number } | null;
}

export type NodeKind = 'dir' | 'file' | 'container' | 'folder' | 'asset' | 'group';
export interface TreeNode {
  name: string;
  path: string;
  kind: NodeKind;
  size?: number;
  ext?: string;
  count?: number;
}
export interface TreePage {
  path: string;
  container: string | null;
  total: number;
  offset: number;
  nodes: TreeNode[];
  named: boolean;
}
export interface SearchHit {
  path: string;
  name: string;
  kind: 'file' | 'asset';
  container?: string;
}
export interface SearchResult {
  hits: SearchHit[];
  truncated: boolean;
  containers: number;
}

export interface TargetInfo {
  type: string;
  label: string;
  defaults: Record<string, unknown>;
}
export type Viewer = 'image' | 'atlas' | 'audio' | 'text' | 'container' | 'none';
export interface Dependency {
  guid: string;
  name: string | null;
}
export interface EntryView {
  path: string;
  viewer: Viewer;
  size: number | null;
  format?: string | null;
  version?: string;
  score?: number;
  alternatives?: { format: string; version?: string; score: number }[];
  name?: string;
  kind?: string;
  targets?: TargetInfo[];
  meta: { name: string | null; kind: string | null; guid: string | null; dependencies: Dependency[] } | null;
  bitmap: { width: number; height: number; depth: number; layers: number; mips: number; pixelFormat: string } | null;
  audio: { channels: number; sampleRate: number }[] | null;
  textTargets: string[];
  hex: boolean;
}
export interface AtlasSprite {
  index: number;
  path: string;
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface Atlas {
  name?: string;
  width: number;
  height: number;
  sprites: AtlasSprite[];
}
export interface HexView {
  size: number;
  rows: { offset: string; hex: string; ascii: string }[];
}

export interface BasketItem {
  from: string;
  to: { type: string; [option: string]: unknown };
  path?: string;
  skip_unsupported?: boolean;
}
export interface OutputView {
  from: string;
  ok: boolean;
  files: string[];
  error: { kind: string; message: string } | null;
}
export interface RowView {
  from: string;
  target: string;
  status: 'waiting' | 'running' | 'finished' | 'skipped';
  outputs: OutputView[];
  error: { kind: string; message: string } | null;
}
export interface JobView {
  id: number;
  started: number;
  state: 'running' | 'done' | 'cancelled';
  done: number;
  total: number;
  rows: RowView[];
}

export type LoginEvent =
  | { state: 'qr'; url: string; qr: { size: number; dark: boolean[] } }
  | { state: 'needsCode'; kind: 'email' | 'device' | null; confirmable: boolean; retry: boolean }
  | { state: 'done'; accountName: string }
  | { state: 'failed'; message: string };

export interface CatalogDepot {
  id: number;
  name?: string | null;
  default: boolean;
}
export interface CatalogVersion {
  id: string;
  name: string;
  date: string;
  type: 'season' | 'event' | 'hotfix' | null;
  group: string | null;
  changed: number[];
}
export interface CatalogGame {
  key: string;
  name: string;
  appId: number;
  depots: CatalogDepot[];
  versions: CatalogVersion[];
}
export interface Catalog {
  games: CatalogGame[];
  problems: { file: string; message: string }[];
  userDir: string | null;
}
export interface Resolved {
  app: number;
  depots: string[];
  leftOut: number[];
}

export type UiEvent =
  | ({ type: 'job' } & JobView)
  | { type: 'log'; level: 'info' | 'warn'; message: string }
  | { type: 'progress'; task: string; done: number; total: number }
  | { type: 'steam'; name: string; data: Record<string, unknown> }
  | { type: 'session'; connected: boolean }
  | ({ type: 'steamLogin' } & LoginEvent);

export interface FsListing {
  path: string;
  parent: string | null;
  dirs: { name: string; path: string }[];
  files?: number;
  containers?: number;
  shortcuts: { home: string | null; cwd: string };
}

// ------------------------------------------------------------- endpoints

export const api = {
  config: () => request<ConfigState>('GET', '/api/config'),
  saveConfig: (config: Config) =>
    request<{ file: string; problem: string | null }>('PUT', '/api/config', config),
  session: () => request<SessionState>('GET', '/api/session'),
  connect: () => request<SessionState>('POST', '/api/session', {}),
  disconnect: () => request<SessionState>('DELETE', '/api/session'),
  steamStatus: () => request<{ loggedIn: boolean; accountName?: string; steamId?: string }>('GET', '/api/steam/status'),
  steamLogout: () => request<{ loggedIn: boolean }>('POST', '/api/steam/logout', {}),
  steamLoginState: () => request<{ running: boolean }>('GET', '/api/steam/login'),
  steamLoginQr: () => request<{ started: boolean }>('POST', '/api/steam/login/qr', {}),
  steamLoginCredentials: (username: string, password: string) =>
    request<{ started: boolean }>('POST', '/api/steam/login/credentials', { username, password }),
  steamLoginCode: (code: string) => request<{ ok: boolean }>('POST', '/api/steam/login/code', { code }),
  steamLoginCancel: () => request<{ ok: boolean }>('DELETE', '/api/steam/login'),
  catalog: () => request<Catalog>('GET', '/api/catalog'),
  resolveVersion: (game: string, version: string, depots?: number[]) =>
    request<Resolved>('POST', '/api/catalog/resolve', { game, version, depots }),
  fs: (path: string) => request<FsListing>('GET', '/api/fs' + qs({ path })),
  tree: (p: {
    path: string;
    view?: 'path' | 'kind';
    group?: string;
    names?: boolean;
    offset?: number;
    limit?: number;
  }) => request<TreePage>('GET', '/api/tree' + qs(p)),
  search: (q: string, scope: string, limit = 200) =>
    request<SearchResult>('GET', '/api/search' + qs({ q, scope, limit })),
  entry: (path: string) => request<EntryView>('GET', '/api/entry' + qs({ path })),
  atlas: (path: string) => request<Atlas>('GET', '/api/preview/atlas' + qs({ path })),
  hex: (path: string) => request<HexView>('GET', '/api/preview/hex' + qs({ path })),
  text: async (path: string, target: string) => {
    const res = await fetch('/api/preview/text' + qs({ path, target }));
    if (!res.ok) {
      const data = await res.json().catch(() => null);
      throw new ApiError(res.status, data?.error?.kind ?? 'Error', data?.error?.message ?? res.statusText);
    }
    return res.text();
  },
  imageUrl: (path: string, mip: number) => '/api/preview/image' + qs({ path, mip }),
  audioUrl: (path: string, track: number) => '/api/preview/audio' + qs({ path, track }),
  atlasImageUrl: (path: string) => '/api/preview/atlas-image' + qs({ path }),
  outputUrl: (file: string, download = false) =>
    '/api/output/' + file.split('/').map(encodeURIComponent).join('/') + (download ? '?download=true' : ''),
  convert: (items: BasketItem[]) => request<JobView>('POST', '/api/convert', { items }),
  jobs: () => request<JobView[]>('GET', '/api/jobs'),
  cancelJob: (id: number) => request<{ ok: boolean }>('DELETE', `/api/jobs/${id}`),
};

/** Fetches a preview into an object URL, so an error body can be shown instead of a broken picture. */
export async function blobUrl(url: string, signal?: AbortSignal): Promise<string> {
  const res = await fetch(url, { signal });
  if (!res.ok) {
    const data = await res.json().catch(() => null);
    throw new ApiError(res.status, data?.error?.kind ?? 'Error', data?.error?.message ?? res.statusText);
  }
  return URL.createObjectURL(await res.blob());
}
