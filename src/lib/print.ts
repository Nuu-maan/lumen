import { fileUrl, svgUrl, type PageSize } from "./api";

// Each distinct page size gets a named @page rule, so Letter pages print on Letter and A4 on A4.
function pageRules(items: { w: number; h: number }[]) {
  const names = new Map<string, string>();
  const rules: string[] = [];
  const nameOf = ({ w, h }: { w: number; h: number }) => {
    const key = `${Math.round(w)}x${Math.round(h)}`;
    let name = names.get(key);
    if (!name) {
      name = `lumen-${key}`;
      names.set(key, name);
      rules.push(`@page ${name} { size: ${w}pt ${h}pt; margin: 0; }`);
    }
    return name;
  };
  const pageNames = items.map(nameOf);
  // Engines without named pages use the first page's size for every sheet.
  const first = items[0];
  if (first) rules.push(`@page { size: ${first.w}pt ${first.h}pt; margin: 0; }`);
  return { css: `@media print { ${rules.join(" ")} }`, pageNames };
}

async function printItems(items: { src: string; w: number; h: number }[], ownPaper: boolean) {
  document.getElementById("print-root")?.remove();
  document.getElementById("print-pages")?.remove();
  const { css, pageNames } = ownPaper ? pageRules(items) : { css: "", pageNames: [] };
  const style = document.createElement("style");
  style.id = "print-pages";
  style.textContent = css;
  document.head.append(style);
  const root = document.createElement("div");
  root.id = "print-root";
  const loads = items.map(({ src, w, h }, i) => {
    const sheet = document.createElement("div");
    sheet.className = "print-sheet";
    if (pageNames[i]) sheet.style.setProperty("page", pageNames[i]);
    sheet.style.width = `${w}pt`;
    sheet.style.height = `${h}pt`;
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
  window.addEventListener(
    "afterprint",
    () => {
      root.remove();
      style.remove();
    },
    { once: true },
  );
  window.print();
}

/** Prints document pages as vector SVG at their own paper size. */
export function printPaged(doc: number, pages: PageSize[]) {
  return printItems(
    pages.map((p, i) => ({ src: svgUrl(doc, i), w: p.w, h: p.h })),
    true,
  );
}

export function printImage(doc: number, natural: { w: number; h: number }) {
  const maxW = 540;
  const maxH = 720;
  const scale = Math.min(1, maxW / natural.w, maxH / natural.h);
  return printItems([{ src: fileUrl(doc), w: natural.w * scale, h: natural.h * scale }], false);
}
