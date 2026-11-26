import { fileUrl, pageUrl, type PageSize } from "./api";

const PRINT_DPI = 150;

async function printImages(items: { src: string; w: number; h: number }[]) {
  document.getElementById("print-root")?.remove();
  const root = document.createElement("div");
  root.id = "print-root";
  const loads = items.map(({ src, w, h }) => {
    const sheet = document.createElement("div");
    sheet.className = "print-sheet";
    const img = new Image();
    img.style.width = `${w}pt`;
    img.style.height = `${h}pt`;
    sheet.append(img);
    root.append(sheet);
    const loaded = new Promise<void>((resolve) => {
      img.onload = img.onerror = () => resolve();
    });
    img.src = src;
    return loaded;
  });
  document.body.append(root);
  await Promise.all(loads);
  window.addEventListener("afterprint", () => root.remove(), { once: true });
  window.print();
}

export function printPaged(doc: number, pages: PageSize[]) {
  return printImages(
    pages.map((p, i) => ({ src: pageUrl(doc, i, Math.min(4096, Math.round((p.w / 72) * PRINT_DPI))), w: p.w, h: p.h })),
  );
}

export function printImage(doc: number, natural: { w: number; h: number }) {
  const maxW = 540;
  const maxH = 720;
  const scale = Math.min(1, maxW / natural.w, maxH / natural.h);
  return printImages([{ src: fileUrl(doc), w: natural.w * scale, h: natural.h * scale }]);
}
