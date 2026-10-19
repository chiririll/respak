import type { NodeKind } from './api';

export function formatSize(bytes: number | undefined | null): string {
  if (bytes === undefined || bytes === null) return '';
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KiB', 'MiB', 'GiB', 'TiB'];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function formatCount(n: number): string {
  return n.toLocaleString('en-US');
}

/** The category that picks an icon and a colour for a file name. */
export type Category = 'dir' | 'container' | 'image' | 'audio' | 'video' | 'model' | 'text' | 'table' | 'other';

const categories: Record<string, Category> = {
  png: 'image', dds: 'image', txtr: 'image', uiia: 'image', uimg: 'image', jpg: 'image', jpeg: 'image', tga: 'image',
  wav: 'audio', asrc: 'audio', ogg: 'audio', mp3: 'audio', mbnk: 'container', mstr: 'container',
  bik: 'video', bk2: 'video', avi: 'video', mov: 'video', y4m: 'video',
  mdl_: 'model', rmdl: 'model', arig: 'model', aseq: 'model', anir: 'model', cast: 'model', mdl: 'model',
  json: 'text', txt: 'text', matl: 'text', rson: 'text', subt: 'text', cfg: 'text', ini: 'text',
  csv: 'table', dtbl: 'table',
  rpak: 'container', starpak: 'container', vpk: 'container',
};

export function categoryOf(name: string, kind: NodeKind): Category {
  if (kind === 'dir' || kind === 'folder') return 'dir';
  if (kind === 'container') return 'container';
  if (kind === 'group') return categories[name] ?? 'other';
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : name.toLowerCase();
  return categories[ext] ?? 'other';
}

export const categoryColor: Record<Category, string> = {
  dir: 'var(--muted)',
  container: 'var(--accent)',
  image: 'var(--info)',
  audio: '#c78bff',
  video: '#ff8bd0',
  model: 'var(--ok)',
  text: 'var(--warn)',
  table: 'var(--warn)',
  other: 'var(--faint)',
};

export function basename(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

export function dirname(path: string): string {
  const i = path.lastIndexOf('/');
  return i < 0 ? '' : path.slice(0, i);
}

export function formatTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
