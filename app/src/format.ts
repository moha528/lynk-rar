const UNITS = ["o", "Ko", "Mo", "Go", "To"];

export function size(bytes: number): string {
  let v = bytes;
  let unit = 0;
  while (v >= 1024 && unit < UNITS.length - 1) {
    v /= 1024;
    unit++;
  }
  if (unit === 0) return `${bytes} o`;
  return `${v.toLocaleString("fr-FR", { maximumFractionDigits: v < 10 ? 1 : 0 })} ${UNITS[unit]}`;
}

const dateFmt = new Intl.DateTimeFormat("fr-FR", { dateStyle: "short", timeStyle: "short" });

export function date(unix: number | null): string {
  return unix === null ? "" : dateFmt.format(new Date(unix * 1000));
}

export function ratio(size: number, packed: number | null): string {
  if (packed === null || size === 0) return "";
  return `${Math.round((packed / size) * 100)} %`;
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function parentDir(path: string): string {
  const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return i > 0 ? path.slice(0, i) : path;
}

/** Type affiché dans la colonne « Type », comme l'Explorateur. */
export function kind(name: string, isDir: boolean): string {
  if (isDir) return "Dossier";
  const dot = name.lastIndexOf(".");
  return dot > 0 ? `Fichier ${name.slice(dot + 1).toUpperCase()}` : "Fichier";
}
