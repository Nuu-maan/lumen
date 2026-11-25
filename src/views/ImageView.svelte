<script lang="ts">
  import { onMount } from "svelte";
  import { fileUrl } from "../lib/api";
  import type { ImageState } from "../lib/image.svelte";

  let { doc, state: s, name }: { doc: number; state: ImageState; name: string } = $props();

  let scroller: HTMLDivElement;
  let viewW = $state(0);
  let viewH = $state(0);

  const rotated = $derived(s.rotation % 180 !== 0);
  const box = $derived.by(() => {
    const n = s.natural;
    if (!n) return null;
    const w = rotated ? n.h : n.w;
    const h = rotated ? n.w : n.h;
    return { w, h };
  });

  $effect(() => {
    if (!box || !viewW || !s.fit) return;
    const fit = Math.min((viewW - 32) / box.w, (viewH - 32) / box.h);
    s.zoom = Math.max(0.1, Math.min(s.fit === "page" ? 1 : 8, s.fit === "width" ? (viewW - 32) / box.w : fit));
  });

  function onload(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    s.natural = { w: img.naturalWidth || 512, h: img.naturalHeight || 512 };
  }

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    s.setZoom(s.zoom * Math.exp(-e.deltaY * 0.0025));
  }

  onMount(() => {
    const ro = new ResizeObserver(() => {
      viewW = scroller.clientWidth;
      viewH = scroller.clientHeight;
    });
    ro.observe(scroller);
    return () => ro.disconnect();
  });
</script>

<div class="scroller" bind:this={scroller} onwheel={onWheel} tabindex="-1" role="document" aria-label={name}>
  <div
    class="canvas"
    style:width={box ? `${Math.max(viewW, box.w * s.zoom + 32)}px` : "100%"}
    style:height={box ? `${Math.max(viewH, box.h * s.zoom + 32)}px` : "100%"}
  >
    <div class="frame" style:width={box ? `${box.w * s.zoom}px` : "auto"} style:height={box ? `${box.h * s.zoom}px` : "auto"}>
      <img
        src={fileUrl(doc)}
        alt={name}
        draggable="false"
        {onload}
        onerror={() => (s.failed = true)}
        style:width={s.natural ? `${s.natural.w * s.zoom}px` : "auto"}
        style:height={s.natural ? `${s.natural.h * s.zoom}px` : "auto"}
        style:transform="translate(-50%, -50%) rotate({s.rotation}deg)"
        class:pixelated={s.zoom >= 3}
        class:hidden={!s.natural}
      />
    </div>
  </div>
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
    outline: none;
  }
  .canvas {
    display: grid;
    place-items: center;
  }
  .frame {
    position: relative;
    background-color: #fff;
    background-image: conic-gradient(#e6e6e6 25%, #fff 0 50%, #e6e6e6 0 75%, #fff 0);
    background-size: 16px 16px;
    box-shadow: var(--shadow-page);
  }
  img {
    position: absolute;
    left: 50%;
    top: 50%;
    max-width: none;
  }
  .pixelated {
    image-rendering: pixelated;
  }
  .hidden {
    visibility: hidden;
  }
</style>
