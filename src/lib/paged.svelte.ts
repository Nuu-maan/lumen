import { api, type OutlineItem, type PageSize, type Rect } from "./api";

export const PT_TO_PX = 96 / 72;
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 8;
const ZOOM_STEPS = [0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5, 6, 8];

export type Fit = "width" | "page" | null;
export type Hit = { page: number; rect: Rect };

export class PagedState {
  readonly doc: number;
  readonly pages: PageSize[];
  readonly reflowable: boolean;
  zoom = $state(1);
  fit = $state<Fit>("width");
  rotation = $state(0);
  mode = $state<"scroll" | "single">("scroll");
  page = $state(0);
  invert = $state(false);
  hits = $state<Hit[]>([]);
  hitIndex = $state(-1);
  searching = $state(false);
  query = $state("");
  outline = $state<OutlineItem[] | null>(null);
  jump = $state<{ page: number; y?: number; seq: number } | null>(null);
  private jumpSeq = 0;

  constructor(doc: number, pages: PageSize[], reflowable: boolean) {
    this.doc = doc;
    this.pages = pages;
    this.reflowable = reflowable;
  }

  get pageCount() {
    return this.pages.length;
  }

  goTo(page: number, y?: number) {
    const clamped = Math.max(0, Math.min(this.pages.length - 1, page));
    this.page = clamped;
    this.jump = { page: clamped, y, seq: ++this.jumpSeq };
  }

  setZoom(zoom: number) {
    this.fit = null;
    this.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom));
  }

  zoomStep(direction: 1 | -1) {
    const z = this.zoom;
    const next = direction > 0 ? ZOOM_STEPS.find((s) => s > z + 0.001) : [...ZOOM_STEPS].reverse().find((s) => s < z - 0.001);
    this.setZoom(next ?? z);
  }

  rotate() {
    this.rotation = (this.rotation + 90) % 360;
  }

  async loadOutline() {
    if (this.outline === null) this.outline = await api.outline(this.doc).catch(() => []);
  }

  search(query: string) {
    this.query = query;
    this.hits = [];
    this.hitIndex = -1;
    if (!query.trim()) {
      this.searching = false;
      void api.search(this.doc, "", 0, () => {});
      return;
    }
    this.searching = true;
    const from = this.page;
    const found: Hit[] = [];
    let flush = 0;
    const publish = () => {
      flush = 0;
      found.sort((a, b) => a.page - b.page || a.rect.y - b.rect.y || a.rect.x - b.rect.x);
      this.hits = found.slice();
      if (this.hitIndex < 0 && this.hits.length) {
        const first = this.hits.findIndex((h) => h.page >= from);
        this.selectHit(first < 0 ? 0 : first);
      }
    };
    void api.search(this.doc, query, from, (event) => {
      if (this.query !== query) return;
      if (event.type === "hits") {
        for (const rect of event.rects) found.push({ page: event.page, rect });
        if (!flush) flush = requestAnimationFrame(publish);
      } else {
        if (flush) cancelAnimationFrame(flush);
        publish();
        this.searching = false;
      }
    });
  }

  selectHit(index: number) {
    if (!this.hits.length) return;
    this.hitIndex = (index + this.hits.length) % this.hits.length;
    const hit = this.hits[this.hitIndex];
    this.goTo(hit.page, hit.rect.y);
  }
}
