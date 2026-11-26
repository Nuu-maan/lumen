export function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

const mac = /Mac|iPhone|iPad/.test(navigator.platform);

export const kbd = (keys: string) => (mac ? (keys.startsWith("Shift+") ? "⇧⌘" + keys.slice(6) : "⌘" + keys) : "Ctrl+" + keys);
