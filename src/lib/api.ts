import { invoke, Channel } from "@tauri-apps/api/core";

export type Family = "document" | "spreadsheet" | "presentation" | "drawing" | "book" | "image" | "text" | "other";

export interface Format {
  id: string;
  label: string;
  exts: string[];
  engine: string;
  family: Family;
}

export interface FileInfo {
  path: string;
  name: string;
  size: number;
  format: Format;
}

export interface PageSize {
  w: number;
  h: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface TextLine {
  text: string;
  rect: Rect;
}

export interface Link {
  rect: Rect;
  page: number | null;
  uri: string | null;
}

export interface OutlineItem {
  title: string;
  page: number | null;
  children: OutlineItem[];
}

export type PagedResult =
  | { status: "ok"; doc: number; pages: PageSize[]; reflowable: boolean }
  | { status: "needsPassword" }
  | { status: "wrongPassword" }
  | { status: "error"; message: string };

export interface Lines {
  start: number;
  lines: string[];
  total: number;
  complete: boolean;
  tokens: [number, number][][] | null;
  highlighting: boolean;
}

export interface Email {
  subject: string;
  from: string;
  to: string;
  cc: string;
  date: string;
  html: string;
  attachments: { index: number; name: string; size: number }[];
}

export interface ArchiveEntry {
  index: number;
  path: string;
  size: number;
  packed: number;
  dir: boolean;
}

export type Converter = "libreOffice" | "msOffice";

export interface Fast {
  html: string;
  notes: string[];
}

export const converterLabel: Record<Converter, string> = {
  libreOffice: "High fidelity (LibreOffice)",
  msOffice: "Exact (MS Office)",
};

export type Opened =
  | { kind: "office"; file: FileInfo; fast: Fast | null; encrypted: boolean; converter: Converter }
  | ({ kind: "paged"; file: FileInfo; engine: string; notes?: string[] | null } & PagedResult)
  | { kind: "text"; file: FileInfo; doc: number; first: Lines; language: string }
  | { kind: "html"; file: FileInfo; html: string }
  | { kind: "email"; file: FileInfo; doc: number; email: Email }
  | { kind: "image"; file: FileInfo; doc: number }
  | { kind: "archive"; file: FileInfo; doc: number; entries: ArchiveEntry[] }
  | { kind: "unsupported"; file: FileInfo | null; message: string };

export type SearchEvent = { type: "hits"; page: number; rects: Rect[] } | { type: "done" };

const isWindows = navigator.userAgent.includes("Windows");
export const protocolBase = isWindows ? "http://lumen.localhost" : "lumen://localhost";

export const tileUrl = (doc: number, page: number, scale: number, x: number, y: number, w: number, h: number) =>
  `${protocolBase}/tile/${doc}/${page}/${Math.round(scale * 1000)}/${x}/${y}/${w}/${h}`;

export const fileUrl = (doc: number) => `${protocolBase}/file/${doc}`;

export const pageUrl = (doc: number, page: number, width: number) => `${protocolBase}/page/${doc}/${page}/${width}`;

export const api = {
  ready: () => invoke<string[]>("app_ready"),
  benchMark: (name: string, ms: number) => invoke<void>("bench_mark", { name, ms }),
  open: (path: string, password?: string) =>
    invoke<Opened>("open_document", { path, password, preferMsOffice: localStorage.getItem("lumen.msoffice") !== "0" }),
  officeExact: (path: string, password: string | undefined, converter: Converter) =>
    invoke<PagedResult>("office_exact", { path, password, converter }),
  msOfficeAvailable: () => invoke<boolean>("ms_office_available"),
  close: (doc: number) => invoke<void>("close_document", { doc }),
  setViewport: (doc: number, first: number, last: number, scale: number) =>
    invoke<void>("set_viewport", { doc, first, last, scale }),
  pageText: (doc: number, page: number) => invoke<TextLine[]>("page_text", { doc, page }),
  pageLinks: (doc: number, page: number) => invoke<Link[]>("page_links", { doc, page }),
  textLines: (doc: number, start: number, count: number) => invoke<Lines>("text_lines", { doc, start, count }),
  textSearch: (doc: number, needle: string) => invoke<[number, number][]>("text_search", { doc, needle }),
  extract: (doc: number, index: number) => invoke<string>("extract_item", { doc, index }),
  outline: (doc: number) => invoke<OutlineItem[]>("outline", { doc }),
  search: (doc: number, needle: string, fromPage: number, onEvent: (e: SearchEvent) => void) => {
    const channel = new Channel<SearchEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("search", { doc, needle, fromPage, onEvent: channel });
  },
};
