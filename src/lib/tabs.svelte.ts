import { api, converterLabel, type ArchiveEntry, type Converter, type Email, type Fast, type FileInfo, type Lines, type PagedResult, type SheetInfo } from "./api";
import { ImageState } from "./image.svelte";
import { PagedState } from "./paged.svelte";

const TEXT_HIT_LIMIT = 50_000;

export class TextFind {
  query = $state("");
  hits = $state<[number, number][]>([]);
  index = $state(-1);
  searching = $state(false);
  capped = $state(false);

  constructor(private doc: number) {}

  async search(query: string) {
    this.query = query;
    this.hits = [];
    this.index = -1;
    this.capped = false;
    if (!query) return;
    this.searching = true;
    const hits = await api.textSearch(this.doc, query, TEXT_HIT_LIMIT).catch(() => []);
    if (this.query !== query) return;
    this.hits = hits;
    this.capped = hits.length >= TEXT_HIT_LIMIT;
    this.index = hits.length ? 0 : -1;
    this.searching = false;
  }

  step(direction: 1 | -1) {
    if (this.hits.length) this.index = (this.index + direction + this.hits.length) % this.hits.length;
  }
}

export class SheetState {
  readonly doc: number;
  readonly names: string[];
  readonly converter: Converter | null;
  sheet = $state(0);
  infos = $state<Record<number, SheetInfo>>({});
  mode = $state<"grid" | "print">("grid");
  print = $state<PagedState | "converting" | { error: string } | null>(null);

  constructor(doc: number, names: string[], info: SheetInfo, converter: Converter | null) {
    this.doc = doc;
    this.names = names;
    this.converter = converter;
    this.infos[0] = info;
  }

  async select(sheet: number) {
    this.sheet = sheet;
    if (!this.infos[sheet]) this.infos[sheet] = await api.sheetInfo(this.doc, sheet).catch(() => ({ rows: 0, cols: 0, frozenRows: 0, frozenCols: 0 }));
  }

  async showPrint(path: string) {
    this.mode = "print";
    if (this.print !== null && !(typeof this.print === "object" && "error" in this.print)) return;
    if (!this.converter) return;
    this.print = "converting";
    try {
      const result = await api.officeExact(path, undefined, this.converter);
      this.print = result.status === "ok" ? new PagedState(result.doc, result.pages, result.reflowable) : { error: "This workbook is password protected." };
    } catch (e) {
      this.print = { error: String(e) === "not-installed" ? "Install LibreOffice to see the print layout." : String(e) };
    }
  }
}

export type TabView =
  | { kind: "loading" }
  | { kind: "paged"; state: PagedState; engine: string; notes?: string[]; swapped?: boolean }
  | { kind: "office"; fast: Fast | null; converter: Converter; status: "converting" | "notInstalled" | "failed"; message?: string }
  | { kind: "text"; doc: number; first: Lines; language: string; find: TextFind }
  | { kind: "html"; html: string }
  | { kind: "sheet"; state: SheetState }
  | { kind: "email"; doc: number; email: Email }
  | { kind: "image"; doc: number; state: ImageState }
  | { kind: "archive"; doc: number; entries: ArchiveEntry[] }
  | { kind: "password"; wrong: boolean; office?: Converter }
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

const basename = (p: string) => p.split(/[\\/]/).pop() || p;

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
  const existing = store.tabs.find((t) => t.path === path);
  if (existing) {
    store.active = existing.id;
    if (existing.view.kind === "error") await load(existing.id);
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
    if ("doc" in result && typeof result.doc === "number") void api.close(result.doc);
    return;
  }
  t.file = result.file;
  const started = openTimes.get(id);
  if (started !== undefined && "doc" in result && typeof result.doc === "number") openTimes.set(-result.doc, started);
  if (result.kind === "unsupported") {
    t.view = { kind: "error", message: result.message };
  } else if (result.kind === "office") {
    if (result.encrypted && !password) {
      t.view = { kind: "password", wrong: false, office: result.converter };
      return;
    }
    t.view = { kind: "office", fast: result.fast, converter: result.converter, status: "converting" };
    if (started !== undefined && result.fast) {
      requestAnimationFrame(() => void api.benchMark("office_fast", performance.now() - started));
    }
    await exact(id, password, result.converter, result.fast?.notes);
  } else if (result.kind === "text") {
    t.doc = result.doc;
    t.view = { kind: "text", doc: result.doc, first: result.first, language: result.language, find: new TextFind(result.doc) };
  } else if (result.kind === "sheet") {
    t.doc = result.doc;
    t.view = { kind: "sheet", state: new SheetState(result.doc, result.names, result.info, result.converter) };
  } else if (result.kind === "html") {
    t.view = { kind: "html", html: result.html };
  } else if (result.kind === "email") {
    t.doc = result.doc;
    t.view = { kind: "email", doc: result.doc, email: result.email };
  } else if (result.kind === "image") {
    t.doc = result.doc;
    t.view = { kind: "image", doc: result.doc, state: new ImageState() };
  } else if (result.kind === "archive") {
    t.doc = result.doc;
    t.view = { kind: "archive", doc: result.doc, entries: result.entries };
  } else {
    applyPaged(t, result, result.engine, result.notes ?? undefined);
  }
}

function applyPaged(t: Tab, result: PagedResult, engine: string, notes?: string[], swapped = false, office?: Converter, triedPassword = false) {
  if (result.status === "ok") {
    t.doc = result.doc;
    t.view = { kind: "paged", state: new PagedState(result.doc, result.pages, result.reflowable), engine, notes, swapped };
  } else if (result.status === "needsPassword" || result.status === "wrongPassword") {
    t.view = { kind: "password", wrong: result.status === "wrongPassword" || triedPassword, office };
  } else {
    t.view = { kind: "error", message: result.message };
  }
}

async function exact(id: number, password: string | undefined, converter: Converter, notes?: string[]) {
  const tab = store.tabs.find((t) => t.id === id);
  if (!tab) return;
  let result: PagedResult;
  try {
    result = await api.officeExact(tab.path, password, converter);
  } catch (e) {
    const t = store.tabs.find((x) => x.id === id);
    if (t?.view.kind === "office") {
      t.view = { ...t.view, status: String(e) === "not-installed" ? "notInstalled" : "failed", message: String(e) };
    }
    return;
  }
  const t = store.tabs.find((x) => x.id === id);
  if (!t) {
    if (result.status === "ok") void api.close(result.doc);
    return;
  }
  if (result.status === "error" && t.view.kind === "office" && t.view.fast) {
    t.view = { ...t.view, status: "failed", message: result.message };
    return;
  }
  const started = openTimes.get(id);
  if (started !== undefined && result.status === "ok") openTimes.set(-result.doc, started);
  applyPaged(t, result, converterLabel[converter], notes, t.view.kind === "office", converter, !!password);
}

export async function retryOffice(id: number, password?: string) {
  const t = store.tabs.find((x) => x.id === id);
  if (!t) return;
  if (t.view.kind === "password" && t.view.office) {
    const converter = t.view.office;
    await exact(id, password, password ? "libreOffice" : converter);
  } else {
    await load(id, password);
  }
}

export function closeTab(id: number) {
  const index = store.tabs.findIndex((t) => t.id === id);
  if (index < 0) return;
  const [tab] = store.tabs.splice(index, 1);
  if (tab.doc !== null) void api.close(tab.doc);
  if (tab.view.kind === "sheet" && tab.view.state.print instanceof PagedState) void api.close(tab.view.state.print.doc);
  if (store.active === id) store.active = store.tabs[Math.min(index, store.tabs.length - 1)]?.id ?? 0;
}

export function cycleTab(direction: 1 | -1) {
  if (store.tabs.length < 2) return;
  const index = store.tabs.findIndex((t) => t.id === store.active);
  store.active = store.tabs[(index + direction + store.tabs.length) % store.tabs.length].id;
}

export async function openExtracted(doc: number, index: number) {
  const path = await api.extract(doc, index);
  await openPath(path);
}
