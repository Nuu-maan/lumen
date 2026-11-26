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

export type Opened =
  | ({ kind: "paged"; file: FileInfo; engine: string } & PagedResult)
  | { kind: "unsupported"; file: FileInfo | null; message: string };

export type SearchEvent = { type: "hits"; page: number; rects: Rect[] } | { type: "done" };

const isWindows = navigator.userAgent.includes("Windows");
export const protocolBase = isWindows ? "http://lumen.localhost" : "lumen://localhost";

export const tileUrl = (doc: number, page: number, scale: number, x: number, y: number, w: number, h: number) =>
  `${protocolBase}/tile/${doc}/${page}/${Math.round(scale * 1000)}/${x}/${y}/${w}/${h}`;

export const pageUrl = (doc: number, page: number, width: number) => `${protocolBase}/page/${doc}/${page}/${width}`;

export const api = {
  ready: () => invoke<string[]>("app_ready"),
  benchMark: (name: string, ms: number) => invoke<void>("bench_mark", { name, ms }),
  open: (path: string, password?: string) => invoke<Opened>("open_document", { path, password }),
  close: (doc: number) => invoke<void>("close_document", { doc }),
  setViewport: (doc: number, first: number, last: number, scale: number) =>
    invoke<void>("set_viewport", { doc, first, last, scale }),
  pageText: (doc: number, page: number) => invoke<TextLine[]>("page_text", { doc, page }),
  pageLinks: (doc: number, page: number) => invoke<Link[]>("page_links", { doc, page }),
  outline: (doc: number) => invoke<OutlineItem[]>("outline", { doc }),
  search: (doc: number, needle: string, fromPage: number, onEvent: (e: SearchEvent) => void) => {
    const channel = new Channel<SearchEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("search", { doc, needle, fromPage, onEvent: channel });
  },
};
