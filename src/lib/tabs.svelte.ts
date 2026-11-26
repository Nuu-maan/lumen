import { api, type FileInfo } from "./api";
import { PagedState } from "./paged.svelte";

export type TabView =
  | { kind: "loading" }
  | { kind: "paged"; state: PagedState; engine: string }
  | { kind: "password"; wrong: boolean }
  | { kind: "error"; message: string };

export interface Tab {
  id: number;
  path: string;
  name: string;
  file: FileInfo | null;
  view: TabView;
  doc: number | null;
}

const RECENT_KEY = "lumen.recent";
let nextId = 1;

export const store = $state({
  tabs: [] as Tab[],
  active: 0,
  recent: JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]") as string[],
});

export const activeTab = () => store.tabs.find((t) => t.id === store.active) ?? null;

const basename = (p: string) => p.split(/[\/]/).pop() || p;

function remember(path: string) {
  store.recent = [path, ...store.recent.filter((p) => p !== path)].slice(0, 12);
  localStorage.setItem(RECENT_KEY, JSON.stringify(store.recent));
}

export function forgetRecent(path: string) {
  store.recent = store.recent.filter((p) => p !== path);
  localStorage.setItem(RECENT_KEY, JSON.stringify(store.recent));
}

export const openTimes = new Map<number, number>();

export async function openPath(path: string) {
  const started = performance.now();
  const existing = store.tabs.find((t) => t.path === path && t.view.kind !== "error");
  if (existing) {
    store.active = existing.id;
    return;
  }
  const tab: Tab = { id: nextId++, path, name: basename(path), file: null, view: { kind: "loading" }, doc: null };
  store.tabs.push(tab);
  openTimes.set(tab.id, started);
  store.active = tab.id;
  remember(path);
  await load(tab.id);
}

export async function load(id: number, password?: string) {
  const find = () => store.tabs.find((t) => t.id === id);
  const tab = find();
  if (!tab) return;
  if (tab.view.kind !== "password") tab.view = { kind: "loading" };
  let result;
  try {
    result = await api.open(tab.path, password);
  } catch (e) {
    result = { kind: "unsupported" as const, file: null, message: String(e) };
  }
  const t = find();
  if (!t) {
    if (result.kind === "paged" && result.status === "ok") void api.close(result.doc);
    return;
  }
  t.file = result.file;
  if (result.kind === "unsupported") {
    t.view = { kind: "error", message: result.message };
  } else if (result.status === "ok") {
    t.doc = result.doc;
    const started = openTimes.get(id);
    if (started !== undefined) openTimes.set(-result.doc, started);
    t.view = { kind: "paged", state: new PagedState(result.doc, result.pages, result.reflowable), engine: result.engine };
  } else if (result.status === "needsPassword" || result.status === "wrongPassword") {
    t.view = { kind: "password", wrong: result.status === "wrongPassword" };
  } else {
    t.view = { kind: "error", message: result.message };
  }
}

export function closeTab(id: number) {
  const index = store.tabs.findIndex((t) => t.id === id);
  if (index < 0) return;
  const [tab] = store.tabs.splice(index, 1);
  if (tab.doc !== null) void api.close(tab.doc);
  if (store.active === id) store.active = store.tabs[Math.min(index, store.tabs.length - 1)]?.id ?? 0;
}

export function cycleTab(direction: 1 | -1) {
  if (store.tabs.length < 2) return;
  const index = store.tabs.findIndex((t) => t.id === store.active);
  store.active = store.tabs[(index + direction + store.tabs.length) % store.tabs.length].id;
}
