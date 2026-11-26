import type { Fit } from "./paged.svelte";

const STEPS = [0.1, 0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4, 6, 8];

export class ImageState {
  zoom = $state(1);
  fit = $state<Fit>("page");
  rotation = $state(0);
  natural = $state<{ w: number; h: number } | null>(null);
  failed = $state(false);

  setZoom(zoom: number) {
    this.fit = null;
    this.zoom = Math.max(0.1, Math.min(8, zoom));
  }

  zoomStep(direction: 1 | -1) {
    const z = this.zoom;
    const next = direction > 0 ? STEPS.find((s) => s > z + 0.001) : [...STEPS].reverse().find((s) => s < z - 0.001);
    this.setZoom(next ?? z);
  }

  rotate() {
    this.rotation = (this.rotation + 90) % 360;
  }
}
