// The lazy tree: one level per request, paged, rebuilt into a flat list for the virtual scroller.

import { api, type TreeNode } from './api';
import { errorText } from './util';

export const PAGE = 500;

export class Row {
  expanded = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);
  children = $state.raw<Row[] | null>(null);
  total = $state(0);
  /** On a container: how its entries are laid out. */
  view = $state<'path' | 'kind'>('path');
  /** On a container: whether the names stored in the pak were read. */
  names = $state(false);

  constructor(
    public node: TreeNode,
    public depth: number,
    public parent: Row | null,
  ) {}

  get expandable(): boolean {
    return this.node.kind !== 'file' && this.node.kind !== 'asset';
  }

  /** The container this row is, or is inside. */
  container(): Row | null {
    for (let r: Row | null = this; r; r = r.parent) if (r.node.kind === 'container') return r;
    return null;
  }

  get hasMore(): boolean {
    return !!this.children && this.children.length < this.total;
  }
}

export type Flat = { type: 'row'; row: Row } | { type: 'more'; row: Row } | { type: 'note'; row: Row; text: string };

function flatten(row: Row, out: Flat[]) {
  if (!row.expanded) return;
  if (row.error) out.push({ type: 'note', row, text: row.error });
  if (row.children) {
    for (const child of row.children) {
      out.push({ type: 'row', row: child });
      flatten(child, out);
    }
    if (row.hasMore) out.push({ type: 'more', row });
    else if (row.children.length === 0 && !row.error && !row.loading) {
      out.push({ type: 'note', row, text: 'empty' });
    }
  }
}

export class TreeModel {
  root = new Row({ name: '', path: '', kind: 'dir' }, -1, null);
  private version = $state(0);
  flat = $derived.by<Flat[]>(() => {
    void this.version;
    const out: Flat[] = [];
    flatten(this.root, out);
    return out;
  });

  constructor() {
    this.root.expanded = true;
  }

  private touch() {
    this.version++;
  }

  reset() {
    this.root = new Row({ name: '', path: '', kind: 'dir' }, -1, null);
    this.root.expanded = true;
    this.touch();
  }

  /** Loads the first page of the root. */
  async start() {
    await this.load(this.root, false);
  }

  async load(row: Row, more: boolean) {
    if (row.loading) return;
    row.loading = true;
    row.error = null;
    this.touch();
    try {
      const container = row.container();
      const offset = more && row.children ? row.children.length : 0;
      const kind = row.node.kind;
      const page = await api.tree({
        path: row.node.path,
        view: kind === 'group' ? 'kind' : kind === 'container' ? row.view : container?.view,
        group: kind === 'group' ? row.node.ext : undefined,
        names: container ? container.names : undefined,
        offset,
        limit: PAGE,
      });
      if (kind === 'container') row.names = page.named;
      const made = page.nodes.map((n) => new Row(n, row.depth + 1, row));
      row.children = more && row.children ? [...row.children, ...made] : made;
      row.total = page.total;
    } catch (e) {
      row.error = errorText(e);
      if (!more) row.children = null;
    } finally {
      row.loading = false;
      this.touch();
    }
  }

  async toggle(row: Row) {
    if (!row.expandable) return;
    if (row.expanded) {
      row.expanded = false;
      this.touch();
      return;
    }
    row.expanded = true;
    this.touch();
    if (!row.children) await this.load(row, false);
  }

  async expand(row: Row) {
    if (!row.expanded) await this.toggle(row);
  }

  /** Switches a container between path view and kind view. */
  async setView(row: Row, view: 'path' | 'kind') {
    if (row.view === view && row.children) return;
    row.view = view;
    row.children = null;
    row.expanded = true;
    await this.load(row, false);
  }

  /** Reads the names stored in the pak (slow) and lays the entries out by them. */
  async readNames(row: Row) {
    row.names = true;
    row.children = null;
    row.expanded = true;
    await this.load(row, false);
  }

  /** Opens the ancestors of `path` and returns its row, or `null` if the tree cannot get there. */
  async reveal(path: string): Promise<Row | null> {
    const parts = path.split('/').filter(Boolean);
    let row = this.root;
    for (let i = 1; i <= parts.length; i++) {
      const prefix = parts.slice(0, i).join('/');
      if (row.node.kind === 'container' && row.view === 'kind') await this.setView(row, 'path');
      await this.expand(row);
      if (!row.children) return null;
      const find = () => row.children!.find((c) => c.node.path === prefix && c.node.kind !== 'group');
      let child = find();
      while (!child && row.hasMore) {
        await this.load(row, true);
        child = find();
      }
      if (!child) return null;
      row = child;
    }
    return row;
  }

  indexOf(row: Row): number {
    return this.flat.findIndex((f) => f.type === 'row' && f.row === row);
  }
}
