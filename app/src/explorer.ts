// Fenêtre principale : un explorateur de fichiers où les archives s'ouvrent comme
// des dossiers. Quatre emplacements : l'Accueil, un dossier du disque, l'intérieur
// d'une archive, les paramètres. Une seule barre d'adresse va du disque jusque
// dans l'archive.

import { writeText } from "./clipboard";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";

import {
  api,
  ARCHIVE_RE,
  create,
  errorMessage,
  extract,
  isCmdError,
  testArchive,
  update,
  type ArchiveInfo,
  type ExtractMode,
  type ExtractReport,
  type FormatId,
  type FormatInfo,
  type FsEntry,
  type Places,
  type RecentItem,
  type Selection,
  type UpdateRequest,
} from "./api";
import logoUrl from "../app-icon.svg";
import * as fmt from "./format";
import { ListView, type Column } from "./listview";
import { LEVELS, renderSettings } from "./settings-view";
import { settings, updateSettings } from "./settings";
import { Toolbar, type ToolbarItem } from "./toolbar";
import { buildTree, countFiles, find, walk, type TreeNode } from "./tree";
import { alertError, askPassword, button, Glyph, h, icon, menuBelow, modal, showMenu, toast, withProgress, type MenuItem } from "./ui";

// ——— Modèle ———

type Loc =
  | { kind: "home" }
  | { kind: "fs"; path: string }
  | { kind: "archive"; path: string; inner: string }
  | { kind: "settings" };

interface Opened {
  info: ArchiveInfo;
  root: TreeNode;
  password: string | null;
}

/** Une ligne de liste, qu'elle vienne du disque ou d'une archive. */
interface Row {
  /** Disque : chemin complet. Archive : chemin interne. */
  key: string;
  name: string;
  /** Texte affiché : le nom, ou le chemin relatif pendant une recherche. */
  label: string;
  isDir: boolean;
  isArchive: boolean;
  size: number | null;
  packed: number | null;
  modified: number | null;
  encrypted: boolean;
  hidden: boolean;
}

const state = {
  loc: { kind: "home" } as Loc,
  back: [] as Loc[],
  forward: [] as Loc[],
  opened: null as Opened | null,
  fsEntries: [] as FsEntry[],
  fsError: null as string | null,
  filter: "",
  formats: [] as FormatInfo[],
  places: { folders: [], drives: [], home: "" } as Places,
  recent: [] as RecentItem[],
  /** Icônes système par type (`.pdf`, `""`, `/`). */
  icons: new Map<string, string>(),
  iconsAsked: new Set<string>(),
  /** Icônes propres à un fichier (programmes, raccourcis). */
  pathIcons: new Map<string, string>(),
  pathIconsAsked: new Set<string>(),
  /** Pendant un glisser sortant, nos propres fichiers repassent par la fenêtre : on les ignore. */
  draggingOut: false,
  sidebarOpen: false,
};

/** Types dont chaque fichier a sa propre icône. */
const OWN_ICON = /\.(exe|lnk|ico|url|msi|scr|cpl|appref-ms)$/i;

const ui = {
  toolbar: new Toolbar(),
  search: h("input", { class: "field search", type: "search", placeholder: "Rechercher" }),
  nav: h("div", { class: "nav-buttons" }),
  address: h("div", { class: "address", tabIndex: -1 }),
  sidebar: h("nav", { class: "sidebar", "aria-label": "Emplacements" }),
  main: h("main", { class: "content" }),
  status: h("footer", { class: "status" }),
  drop: h("div", { class: "drop-hint" }, icon(Glyph.add), h("span", { class: "drop-text" }, "Déposez pour ouvrir ou archiver")),
  fsList: null as unknown as ListView<Row>,
  archiveList: null as unknown as ListView<Row>,
};

// ——— Démarrage ———

export async function startExplorer(root: HTMLElement, initial: { open?: string; compress?: string[] }): Promise<void> {
  [state.formats, state.places, state.recent] = await Promise.all([
    api.formats(),
    api.places(),
    api.recentList().catch(() => []),
  ]);
  ui.fsList = makeList("fs");
  ui.archiveList = makeList("archive");

  const sidebarToggle = button("", { glyph: "", title: "Afficher ou masquer le volet de navigation", onclick: () => toggleSidebar() });
  sidebarToggle.classList.add("sidebar-toggle");
  ui.search.addEventListener("input", () => {
    state.filter = ui.search.value.trim().toLocaleLowerCase("fr");
    fillLists();
  });
  ui.search.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      ui.search.value = "";
      state.filter = "";
      fillLists();
      currentList()?.focus();
    }
  });

  root.append(
    h("header", { class: "top" }, ui.toolbar.el, h("div", { class: "search-box" }, icon(Glyph.search), ui.search)),
    h("div", { class: "addressbar" }, sidebarToggle, ui.nav, ui.address),
    h("div", { class: "body" }, ui.sidebar, h("div", { class: "content-wrap" }, ui.main, ui.drop)),
    ui.status,
  );
  ui.address.addEventListener("click", (e) => {
    if (!(e.target as HTMLElement).closest(".crumb, .crumb-more")) editAddress();
  });
  // Les segments se replient (« … ») ou réapparaissent quand la largeur change.
  new ResizeObserver(() => {
    if (!ui.address.querySelector("input")) renderAddress();
  }).observe(ui.address);

  // Affichage étroit : le volet de navigation devient un tiroir.
  const narrow = matchMedia("(max-width: 760px)");
  const applyNarrow = () => document.body.classList.toggle("narrow", narrow.matches);
  narrow.addEventListener("change", applyNarrow);
  applyNarrow();
  document.body.classList.toggle("no-sidebar", !settings().sidebar);

  document.addEventListener("keydown", onGlobalKey);
  await listenDragDrop();

  await getCurrentWindow().show();
  if (initial.open) {
    await openPathOrArchive(initial.open);
  } else {
    await go(await startLocation(), { push: false });
  }
  if (initial.compress?.length) await compressDialog(initial.compress);
}

async function startLocation(): Promise<Loc> {
  const s = settings();
  const downloads = state.places.folders.find((f) => /downloads|t[ée]l[ée]chargements/i.test(f.path));
  if (s.start === "downloads" && downloads) return { kind: "fs", path: downloads.path };
  const target = s.start === "custom" ? s.startPath : s.start === "last" ? s.lastPath : "";
  if (target) {
    const loc = await locFromPath(target);
    if (loc) return loc;
  }
  return { kind: "home" };
}

// ——— Navigation ———

function sameLoc(a: Loc, b: Loc): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function locPath(loc: Loc): string {
  if (loc.kind === "fs") return loc.path;
  if (loc.kind === "archive") return loc.inner ? `${loc.path}\\${loc.inner.replaceAll("/", "\\")}` : loc.path;
  return "";
}

function parentFs(path: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, "");
  const i = trimmed.lastIndexOf("\\");
  if (i < 0) return null;
  const parent = trimmed.slice(0, i);
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

/** Chemin tapé ou reçu → emplacement : dossier, archive, ou dossier dans une archive. */
async function locFromPath(raw: string): Promise<Loc | null> {
  let path = raw.trim().replace(/^"|"$/g, "").replace(/\//g, "\\");
  if (/^[A-Za-z]:$/.test(path)) path += "\\";
  const kind = await api.pathKind(path);
  if (kind === "dir") return { kind: "fs", path };
  if (kind === "file") return ARCHIVE_RE.test(path) ? { kind: "archive", path, inner: "" } : null;
  // Chemin introuvable : peut-être un dossier à l'intérieur d'une archive.
  const parts = path.split("\\");
  for (let i = parts.length - 1; i > 0; i--) {
    const head = parts.slice(0, i).join("\\");
    if (ARCHIVE_RE.test(head) && (await api.pathKind(head)) === "file") {
      return { kind: "archive", path: head, inner: parts.slice(i).join("/") };
    }
  }
  return null;
}

/** Ouvre un chemin reçu en argument ou déposé : archive dedans, dossier, sinon l'app par défaut. */
async function openPathOrArchive(path: string): Promise<void> {
  const loc = await locFromPath(path);
  if (loc) {
    // Le dossier de l'archive en dessous dans l'historique : « Précédent » y ramène.
    const parent = loc.kind === "archive" ? parentFs(loc.path) : null;
    if (parent && state.loc.kind === "home") state.loc = { kind: "fs", path: parent };
    await go(loc);
  } else {
    await api.openPath(path).catch((e) => alertError("Impossible d'ouvrir", errorMessage(e)));
  }
}

/** Va à un emplacement. Rien ne change si le chargement échoue ou est annulé. */
async function go(loc: Loc, opts: { push?: boolean; select?: string[] } = {}): Promise<boolean> {
  if (loc.kind === "fs") {
    try {
      state.fsEntries = await api.listDir(loc.path, settings().showHidden);
      state.fsError = null;
    } catch (e) {
      if (state.loc.kind === "fs" && sameLoc(state.loc, loc)) {
        state.fsEntries = [];
        state.fsError = errorMessage(e);
      } else {
        toast(`Accès impossible : ${errorMessage(e)}`);
        return false;
      }
    }
  } else if (loc.kind === "archive") {
    if (!(await ensureArchive(loc.path))) return false;
    if (!find(state.opened!.root, loc.inner)) loc = { ...loc, inner: "" };
  }
  if (opts.push !== false && !sameLoc(loc, state.loc)) {
    state.back.push(state.loc);
    state.forward = [];
  }
  state.loc = loc;
  if (loc.kind === "fs" || loc.kind === "archive") updateSettings({ lastPath: locPath(loc) });
  ui.search.value = "";
  state.filter = "";
  state.sidebarOpen = false;
  document.body.classList.remove("sidebar-open");
  await render(opts.select);
  return true;
}

async function goBack(): Promise<void> {
  const loc = state.back.pop();
  if (!loc) return;
  const from = state.loc;
  if (await go(loc, { push: false, select: selectionAfterReturn(from, loc) })) state.forward.push(from);
  else state.back.push(loc);
}

async function goForward(): Promise<void> {
  const loc = state.forward.pop();
  if (!loc) return;
  const from = state.loc;
  if (await go(loc, { push: false })) state.back.push(from);
  else state.forward.push(loc);
}

/** En revenant au dossier parent, on resélectionne d'où l'on vient. */
function selectionAfterReturn(from: Loc, to: Loc): string[] {
  if (from.kind === "fs" && to.kind === "fs") return [from.path];
  if (from.kind === "archive" && to.kind === "fs") return [from.path];
  if (from.kind === "archive" && to.kind === "archive" && from.path === to.path) return [from.inner.split("/").slice(0, (to.inner ? to.inner.split("/").length : 0) + 1).join("/")];
  return [];
}

async function goUp(): Promise<void> {
  const loc = state.loc;
  if (loc.kind === "fs") {
    const parent = parentFs(loc.path);
    await go(parent ? { kind: "fs", path: parent } : { kind: "home" }, { select: [loc.path] });
  } else if (loc.kind === "archive") {
    if (loc.inner) {
      const i = loc.inner.lastIndexOf("/");
      await go({ ...loc, inner: i < 0 ? "" : loc.inner.slice(0, i) }, { select: [loc.inner] });
    } else {
      const parent = parentFs(loc.path);
      if (parent) await go({ kind: "fs", path: parent }, { select: [loc.path] });
    }
  } else if (loc.kind === "settings") {
    await goBack();
  }
}

async function refresh(): Promise<void> {
  const keep = currentList()?.selectedItems().map((r) => r.key) ?? [];
  if (state.loc.kind === "archive") await reloadArchive();
  if (state.loc.kind === "home") state.places = await api.places();
  await go(state.loc, { push: false, select: keep });
}

// ——— Archives ———

async function ensureArchive(path: string): Promise<boolean> {
  if (state.opened?.info.path === path) return true;
  let password: string | null = null;
  for (let attempt = 0; ; attempt++) {
    try {
      const info = await api.open(path, password);
      state.opened = { info, root: buildTree(info.entries), password };
      void api.recentAdd(path);
      state.recent = [
        { path, name: info.name, size: info.size, opened: Date.now() / 1000 },
        ...state.recent.filter((r) => r.path !== path),
      ].slice(0, 12);
      return true;
    } catch (e) {
      if (isCmdError(e) && e.kind === "password") {
        password = await askPassword(fmt.baseName(path), attempt > 0);
        if (password === null) return false;
        continue;
      }
      await alertError("Impossible d'ouvrir l'archive", errorMessage(e));
      return false;
    }
  }
}

async function reloadArchive(): Promise<void> {
  const opened = state.opened;
  if (!opened) return;
  const info = await api.open(opened.info.path, opened.password);
  state.opened = { info, root: buildTree(info.entries), password: opened.password };
}

function canModify(): boolean {
  return state.loc.kind === "archive" && state.opened !== null && state.opened.info.format !== "rar" && !/\.\d{3}$/.test(state.opened.info.path);
}

async function pickArchive(): Promise<void> {
  const path = await open({
    title: "Ouvrir une archive",
    filters: [{ name: "Archives", extensions: ["zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "tzst", "jar", "001"] }],
  });
  if (typeof path === "string") await openPathOrArchive(path);
}

// ——— Lignes et colonnes ———

function ext(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot).toLowerCase() : "";
}

function rowIcon(r: Row, fsPath: string | null): HTMLElement {
  const src = (fsPath && state.pathIcons.get(fsPath)) || state.icons.get(r.isDir ? "/" : ext(r.name));
  if (src) return h("img", { class: "ficon", src, alt: "" });
  return icon(r.isDir ? Glyph.folder : Glyph.file, r.isDir ? "folder" : "doc");
}

function columns(view: "fs" | "archive"): Column<Row>[] {
  const name: Column<Row> = {
    id: "name",
    label: "Nom",
    width: view === "fs" ? 380 : 360,
    min: 140,
    cell: (r) =>
      h(
        "span",
        { class: `name-cell${r.hidden ? " is-hidden" : ""}` },
        rowIcon(r, view === "fs" ? r.key : null),
        h("span", { class: "ellipsis", title: r.label }, r.label),
        r.encrypted ? icon(Glyph.lock, "lock") : null,
      ),
    text: (r) => r.label,
    sortValue: (r) => r.label,
  };
  const modified: Column<Row> = {
    id: "modified",
    label: "Modifié le",
    width: 150,
    hideBelow: 520,
    cell: (r) => h("span", { class: "muted" }, fmt.date(r.modified)),
    text: (r) => fmt.date(r.modified),
    sortValue: (r) => r.modified ?? 0,
  };
  const kind: Column<Row> = {
    id: "kind",
    label: "Type",
    width: 150,
    hideBelow: 720,
    cell: (r) => h("span", { class: "muted" }, kindLabel(r)),
    text: kindLabel,
    sortValue: kindLabel,
  };
  const size: Column<Row> = {
    id: "size",
    label: "Taille",
    width: 96,
    align: "end",
    hideBelow: 400,
    cell: (r) => (r.size === null || (r.isDir && view === "fs") ? "" : fmt.size(r.size)),
    text: (r) => (r.size === null ? "" : fmt.size(r.size)),
    sortValue: (r) => r.size ?? -1,
  };
  if (view === "fs") return [name, modified, kind, size];
  const packed: Column<Row> = {
    id: "packed",
    label: "Compressé",
    width: 96,
    align: "end",
    hideBelow: 860,
    cell: (r) => h("span", { class: "muted", title: fmt.ratio(r.size ?? 0, r.packed) }, r.packed === null ? "" : fmt.size(r.packed)),
    text: (r) => (r.packed === null ? "" : fmt.size(r.packed)),
    sortValue: (r) => r.packed ?? -1,
  };
  return [name, modified, kind, size, packed];
}

function kindLabel(r: Row): string {
  if (r.isArchive) return `Archive ${ext(r.name).slice(1).toUpperCase()}`;
  return fmt.kind(r.name, r.isDir);
}

function makeList(view: "fs" | "archive"): ListView<Row> {
  const widths = { ...(settings().columns[view] ?? {}) };
  const list = new ListView<Row>(
    {
      key: (r) => r.key,
      name: (r) => r.name,
      isDir: (r) => r.isDir,
      columns: columns(view),
      widths,
      saveWidths: (w) => updateSettings({ columns: { ...settings().columns, [view]: { ...w } } }),
      onActivate: (r) => void activate(r),
      onContextMenu: (rows, e) => showMenu(view === "fs" ? fsMenu(rows) : archiveMenu(rows), { x: e.clientX, y: e.clientY }),
      onSelection: () => {
        renderToolbar();
        renderStatus();
      },
      onDragStart: () => void dragOut(),
      onKey: (e) => {
        if (e.key === "Backspace") {
          e.preventDefault();
          void goUp();
          return true;
        }
        if (e.key === "Delete" && view === "archive") {
          e.preventDefault();
          void removeSelected();
          return true;
        }
        return false;
      },
    },
    { key: "name", asc: true },
  );
  return list;
}

function currentList(): ListView<Row> | null {
  if (state.loc.kind === "fs") return ui.fsList;
  if (state.loc.kind === "archive") return ui.archiveList;
  return null;
}

function fsRows(): Row[] {
  const f = state.filter;
  return state.fsEntries
    .filter((e) => !f || e.name.toLocaleLowerCase("fr").includes(f))
    .map((e) => ({
      key: e.path,
      name: e.name,
      label: e.name,
      isDir: e.isDir,
      isArchive: !e.isDir && ARCHIVE_RE.test(e.name),
      size: e.isDir ? null : e.size,
      packed: null,
      modified: e.modified,
      encrypted: false,
      hidden: e.hidden,
    }));
}

function archiveRows(): Row[] {
  const loc = state.loc;
  const opened = state.opened;
  if (loc.kind !== "archive" || !opened) return [];
  const toRow = (n: TreeNode, label: string): Row => ({
    key: n.path,
    name: n.name,
    label,
    isDir: n.isDir,
    isArchive: false,
    size: n.size,
    packed: n.packed,
    modified: n.modified,
    encrypted: n.encrypted,
    hidden: false,
  });
  if (state.filter) {
    // Recherche dans toute l'archive, chemins complets affichés.
    const out: Row[] = [];
    for (const n of walk(opened.root)) {
      if (n.name.toLocaleLowerCase("fr").includes(state.filter)) out.push(toRow(n, n.path));
      if (out.length >= 10_000) break;
    }
    return out;
  }
  const dir = find(opened.root, loc.inner);
  return [...(dir?.children?.values() ?? [])].map((n) => toRow(n, n.name));
}

function fillLists(select: string[] = []): void {
  if (state.loc.kind === "fs") {
    const rows = fsRows();
    const empty = state.fsError ?? (state.filter ? "Aucun résultat" : "Ce dossier est vide.");
    ui.fsList.setItems(rows, { select, empty });
    void loadIcons(rows, true);
  } else if (state.loc.kind === "archive") {
    const rows = archiveRows();
    ui.archiveList.setItems(rows, { select, empty: state.filter ? "Aucun résultat" : "Ce dossier est vide." });
    void loadIcons(rows, false);
  }
}

/** Demande au shell les icônes manquantes, puis redessine les lignes visibles. */
async function loadIcons(rows: Row[], fs: boolean): Promise<void> {
  const types = [...new Set(rows.map((r) => (r.isDir ? "/" : ext(r.name))))].filter((k) => !state.iconsAsked.has(k));
  const own = fs ? rows.filter((r) => OWN_ICON.test(r.name) && !state.pathIconsAsked.has(r.key)).slice(0, 400).map((r) => r.key) : [];
  if (!types.length && !own.length) return;
  types.forEach((k) => state.iconsAsked.add(k));
  own.forEach((k) => state.pathIconsAsked.add(k));
  const [byType, byPath] = await Promise.all([
    types.length ? api.fileIcons(types).catch(() => ({})) : {},
    own.length ? api.pathIcons(own).catch(() => ({})) : {},
  ]);
  Object.entries(byType as Record<string, string>).forEach(([k, v]) => state.icons.set(k, v));
  Object.entries(byPath as Record<string, string>).forEach(([k, v]) => state.pathIcons.set(k, v));
  currentList()?.refresh();
  if (state.loc.kind === "home") renderMain();
}

async function activate(r: Row): Promise<void> {
  const loc = state.loc;
  if (loc.kind === "fs") {
    if (r.isDir) await go({ kind: "fs", path: r.key });
    else if (r.isArchive) await go({ kind: "archive", path: r.key, inner: "" });
    else await api.openPath(r.key).catch((e) => alertError("Impossible d'ouvrir", errorMessage(e)));
  } else if (loc.kind === "archive") {
    if (r.isDir) await go({ ...loc, inner: r.key });
    else await openEntry(r.key);
  }
}

// ——— Rendu ———

async function render(select: string[] = []): Promise<void> {
  renderToolbar();
  renderNav();
  renderAddress();
  renderSidebar();
  await renderMain(select);
  renderStatus();
  const title = state.loc.kind === "home" ? "Lynk" : state.loc.kind === "settings" ? "Paramètres – Lynk" : `${locName(state.loc)} – Lynk`;
  void getCurrentWindow().setTitle(title);
  ui.search.placeholder = state.loc.kind === "archive" ? `Rechercher dans ${state.opened?.info.name ?? "l'archive"}` : state.loc.kind === "fs" ? `Rechercher dans ${locName(state.loc)}` : "Rechercher";
  ui.search.disabled = state.loc.kind === "home" || state.loc.kind === "settings";
}

function locName(loc: Loc): string {
  if (loc.kind === "fs") return placeName(loc.path) ?? fmt.baseName(loc.path.replace(/\\$/, "")) ?? loc.path;
  if (loc.kind === "archive") return loc.inner ? loc.inner.split("/").pop()! : fmt.baseName(loc.path);
  return "Lynk";
}

function placeName(path: string): string | null {
  const norm = (p: string) => p.replace(/\\$/, "").toLowerCase();
  const all = [...state.places.folders, ...state.places.drives];
  return all.find((p) => norm(p.path) === norm(path))?.name ?? null;
}

async function renderMain(select: string[] = []): Promise<void> {
  const loc = state.loc;
  document.body.dataset.view = loc.kind;
  if (loc.kind === "home") {
    ui.main.replaceChildren(homeView());
  } else if (loc.kind === "settings") {
    ui.main.replaceChildren(await renderSettings(state.formats));
  } else {
    const list = currentList()!;
    if (ui.main.firstChild !== list.el) ui.main.replaceChildren(list.el);
    fillLists(select);
    list.focus();
  }
}

function renderNav(): void {
  const up = state.loc.kind === "fs" || (state.loc.kind === "archive");
  ui.nav.replaceChildren(
    button("", { glyph: Glyph.back, title: "Précédent (Alt+Gauche)", disabled: !state.back.length, onclick: () => void goBack() }),
    button("", { glyph: "", title: "Suivant (Alt+Droite)", disabled: !state.forward.length, onclick: () => void goForward() }),
    button("", { glyph: Glyph.up, title: "Dossier parent (Alt+Haut)", disabled: !up, onclick: () => void goUp() }),
    button("", { glyph: "", title: "Actualiser (F5)", disabled: state.loc.kind === "settings", onclick: () => void refresh() }),
  );
}

interface Crumb {
  label: string;
  loc: Loc;
  glyph?: string;
  img?: string | null;
}

function crumbs(): Crumb[] {
  const loc = state.loc;
  const home: Crumb = { label: "Accueil", loc: { kind: "home" }, glyph: "" };
  if (loc.kind === "home") return [home];
  if (loc.kind === "settings") return [home, { label: "Paramètres", loc, glyph: "" }];
  const fsPath = loc.path;
  const out: Crumb[] = [home];
  // Sous un dossier connu (Téléchargements…) : on part de lui, comme l'Explorateur.
  const known = state.places.folders.find((f) => fsPath.toLowerCase().startsWith(f.path.toLowerCase() + "\\") || fsPath.toLowerCase() === f.path.toLowerCase());
  let base: string;
  if (known) {
    out.push({ label: known.name, loc: { kind: "fs", path: known.path }, img: known.icon });
    base = known.path;
  } else {
    const drive = fsPath.slice(0, 3);
    const d = state.places.drives.find((p) => p.path.toLowerCase() === drive.toLowerCase());
    out.push({ label: d?.name ?? drive, loc: { kind: "fs", path: drive }, img: d?.icon });
    base = drive.replace(/\\$/, "");
  }
  const rest = fsPath.slice(base.length).split("\\").filter(Boolean);
  let acc = base;
  rest.forEach((part, i) => {
    acc = `${acc}\\${part}`;
    const isArchive = loc.kind === "archive" && i === rest.length - 1;
    out.push({
      label: part,
      loc: isArchive ? { kind: "archive", path: acc, inner: "" } : { kind: "fs", path: acc },
      glyph: isArchive ? Glyph.archive : undefined,
      img: isArchive ? state.icons.get(ext(part)) : undefined,
    });
  });
  if (loc.kind === "archive" && loc.inner) {
    const parts = loc.inner.split("/");
    parts.forEach((p, i) => out.push({ label: p, loc: { kind: "archive", path: loc.path, inner: parts.slice(0, i + 1).join("/") } }));
  }
  return out;
}

function renderAddress(): void {
  const list = crumbs();
  const node = (c: Crumb, i: number): HTMLElement[] => {
    const b = h(
      "button",
      { class: `crumb${i === list.length - 1 ? " current" : ""}`, type: "button", onclick: () => void go(c.loc) },
      c.img ? h("img", { class: "crumb-icon", src: c.img, alt: "" }) : c.glyph ? icon(c.glyph) : null,
      h("span", { class: "ellipsis" }, c.label),
    );
    return i === 0 ? [b] : [icon(Glyph.chevron, "crumb-sep"), b];
  };
  ui.address.replaceChildren(...list.flatMap(node));
  // Trop long : les premiers segments se replient dans un menu « … ».
  let hidden = 0;
  while (ui.address.scrollWidth > ui.address.clientWidth && hidden < list.length - 1) {
    hidden++;
    const more = button("", {
      glyph: "",
      title: "Emplacements précédents",
      onclick: () => menuBelow(more, list.slice(0, hidden).map((c) => ({ label: c.label, glyph: c.glyph ?? Glyph.folder, action: () => void go(c.loc) }))),
    });
    more.classList.add("crumb-more");
    ui.address.replaceChildren(more, ...list.slice(hidden).flatMap((c, i) => node(c, i + hidden)));
  }
}

/** Saisie directe d'un chemin (clic dans la barre, Ctrl+L) : dossier, archive ou dossier d'archive. */
function editAddress(): void {
  if (ui.address.querySelector("input")) return;
  const input = h("input", { class: "address-input", value: locPath(state.loc), spellcheck: false, "aria-label": "Adresse" });
  ui.address.replaceChildren(input);
  input.focus();
  input.select();
  let done = false;
  const finish = async (commit: boolean) => {
    if (done) return;
    done = true;
    if (commit && input.value.trim()) {
      const loc = await locFromPath(input.value);
      if (loc) {
        await go(loc);
        return;
      }
      toast(`Emplacement introuvable : ${input.value}`);
    }
    renderAddress();
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") void finish(true);
    if (e.key === "Escape") void finish(false);
  });
  input.addEventListener("blur", () => void finish(false));
}

function renderSidebar(): void {
  const loc = state.loc;
  const current = (p: string) => loc.kind === "fs" && loc.path.toLowerCase().replace(/\\$/, "") === p.toLowerCase().replace(/\\$/, "");
  const item = (label: string, onclick: () => void, active: boolean, img?: string | null, glyph?: string, sub?: string) =>
    h(
      "button",
      { class: `side-item${active ? " active" : ""}`, type: "button", onclick, title: label },
      img ? h("img", { class: "ficon", src: img, alt: "" }) : icon(glyph ?? Glyph.folder),
      h("span", { class: "ellipsis" }, label),
      sub ? h("span", { class: "side-sub" }, sub) : null,
    );
  ui.sidebar.replaceChildren(
    item("Accueil", () => void go({ kind: "home" }), loc.kind === "home", null, ""),
    h("div", { class: "side-sep" }),
    ...state.places.folders.map((f) => item(f.name, () => void go({ kind: "fs", path: f.path }), current(f.path), f.icon)),
    h("div", { class: "side-sep" }),
    ...state.places.drives.map((d) => item(d.name, () => void go({ kind: "fs", path: d.path }), current(d.path), d.icon)),
    h("div", { class: "side-spacer" }),
    item("Paramètres", () => void go({ kind: "settings" }), loc.kind === "settings", null, ""),
  );
}

function toggleSidebar(): void {
  if (document.body.classList.contains("narrow")) {
    state.sidebarOpen = !state.sidebarOpen;
    document.body.classList.toggle("sidebar-open", state.sidebarOpen);
  } else {
    const visible = !settings().sidebar;
    updateSettings({ sidebar: visible });
    document.body.classList.toggle("no-sidebar", !visible);
  }
}

// ——— Accueil ———

function homeView(): HTMLElement {
  const tile = (name: string, img: string | null, onclick: () => void, extra?: Node) =>
    h(
      "button",
      { class: "tile", type: "button", onclick, title: name },
      img ? h("img", { class: "tile-icon", src: img, alt: "" }) : icon(Glyph.folder, "tile-glyph"),
      h("span", { class: "tile-text" }, h("span", { class: "tile-name ellipsis" }, name), extra ?? null),
    );
  const drives = state.places.drives.map((d) => {
    const used = d.total && d.free !== null ? 1 - d.free / d.total : null;
    const info =
      used === null
        ? h("span", { class: "muted small" }, "Aucun média")
        : h(
            "span",
            { class: "drive-info" },
            h("span", { class: `usage${used > 0.9 ? " full" : ""}` }, h("span", { class: "usage-fill", style: `width:${Math.round(used * 100)}%` })),
            h("span", { class: "muted small" }, `${fmt.size(d.free ?? 0)} libres sur ${fmt.size(d.total ?? 0)}`),
          );
    return tile(d.name, d.icon, () => void go({ kind: "fs", path: d.path }), info);
  });

  const recent = state.recent.length
    ? h(
        "section",
        { class: "home-section" },
        h("h2", {}, "Archives récentes"),
        h(
          "div",
          { class: "recent" },
          ...state.recent.map((r) => {
            const remove = button("", {
              glyph: Glyph.cancel,
              title: "Retirer de la liste",
              onclick: (e) => {
                e.stopPropagation();
                state.recent = state.recent.filter((x) => x.path !== r.path);
                void api.recentRemove(r.path);
                renderMain();
              },
            });
            remove.classList.add("recent-remove");
            return h(
              "div",
              { class: "recent-item", title: r.path, onclick: () => void openPathOrArchive(r.path) },
              rowIcon({ name: r.name, isDir: false } as Row, null),
              h("span", { class: "recent-name ellipsis" }, r.name),
              h("span", { class: "recent-dir muted ellipsis" }, fmt.parentDir(r.path)),
              h("span", { class: "muted small recent-size" }, fmt.size(r.size)),
              remove,
            );
          }),
        ),
      )
    : null;
  void loadIcons(state.recent.map((r) => ({ name: r.name, isDir: false, key: r.path }) as Row), false);

  return h(
    "div",
    { class: "home" },
    h(
      "div",
      { class: "home-hero" },
      h("img", { src: logoUrl, class: "home-logo", alt: "" }),
      h("div", {}, h("h1", {}, "Lynk"), h("p", { class: "muted" }, "Parcourez vos fichiers, ouvrez une archive comme un dossier, ou déposez des fichiers ici pour les archiver.")),
      h(
        "div",
        { class: "row home-actions" },
        button("Ouvrir une archive", { variant: "accent", glyph: Glyph.open, onclick: () => void pickArchive() }),
        button("Nouvelle archive", { variant: "standard", glyph: Glyph.archive, onclick: () => void pickAndCompress() }),
      ),
    ),
    h("section", { class: "home-section" }, h("h2", {}, "Accès rapide"), h("div", { class: "tiles" }, ...state.places.folders.map((f) => tile(f.name, f.icon, () => void go({ kind: "fs", path: f.path }))))),
    h("section", { class: "home-section" }, h("h2", {}, "Lecteurs"), h("div", { class: "tiles drives" }, ...drives)),
    recent,
  );
}

// ——— Barre de commandes ———

function selectedRows(): Row[] {
  return currentList()?.selectedItems() ?? [];
}

function renderToolbar(): void {
  const loc = state.loc;
  const sel = selectedRows();
  const settingsTool = { id: "settings", label: "Paramètres", glyph: "", priority: 0, action: () => void go({ kind: "settings" }) };
  let items: ToolbarItem[] = [];
  if (loc.kind === "home") {
    items = [
      { id: "open", label: "Ouvrir une archive", glyph: Glyph.open, priority: 9, action: () => void pickArchive(), title: "Ouvrir une archive (Ctrl+O)" },
      { id: "new", label: "Nouvelle archive", glyph: Glyph.archive, priority: 8, action: () => void pickAndCompress(), title: "Créer une archive (Ctrl+N)" },
      "sep",
      settingsTool,
    ];
  } else if (loc.kind === "fs") {
    const archives = sel.filter((r) => r.isArchive);
    const allArchives = sel.length > 0 && archives.length === sel.length;
    items = [
      {
        id: "compress",
        label: sel.length ? "Ajouter à une archive" : "Nouvelle archive",
        glyph: Glyph.archive,
        accent: sel.length > 0 && !allArchives,
        priority: 9,
        title: "Créer une archive avec la sélection (Ctrl+N)",
        action: () => void (sel.length ? compressDialog(sel.map((r) => r.key), loc.path) : pickAndCompress()),
      },
      {
        id: "extract",
        label: "Extraire",
        glyph: Glyph.extract,
        accent: allArchives,
        disabled: !allArchives,
        priority: 10,
        title: allArchives ? "Extraire les archives sélectionnées" : "Sélectionnez une ou plusieurs archives",
        menu: () => extractItemsFs(archives.map((r) => r.key)),
      },
      {
        id: "test",
        label: "Tester",
        glyph: Glyph.check,
        disabled: archives.length !== 1 || sel.length !== 1,
        priority: 4,
        title: "Vérifier l'intégrité de l'archive",
        action: () => void runTest(archives[0].key, null),
      },
      "sep",
      { id: "open-sel", label: "Ouvrir", glyph: Glyph.open, disabled: sel.length !== 1, priority: 5, action: () => void activate(sel[0]) },
      { id: "reveal", label: "Afficher dans l'Explorateur", glyph: Glyph.reveal, priority: 3, action: () => void api.reveal(sel[0]?.key ?? loc.path) },
      { id: "copy", label: "Copier le chemin", glyph: "", priority: 2, action: () => void copyPaths(sel.length ? sel.map((r) => r.key) : [loc.path]) },
      "sep",
      settingsTool,
    ];
  } else if (loc.kind === "archive") {
    const editable = canModify();
    const readOnly = editable ? undefined : "Archive en lecture seule (RAR ou volumes)";
    items = [
      { id: "extract", label: "Extraire", glyph: Glyph.extract, accent: true, priority: 10, menu: () => extractItemsArchive() },
      { id: "add", label: "Ajouter", glyph: Glyph.add, disabled: !editable, priority: 8, title: readOnly ?? "Ajouter des fichiers dans ce dossier de l'archive", action: () => void pickAndAdd() },
      { id: "delete", label: "Supprimer", glyph: Glyph.delete, disabled: !editable || !sel.length, priority: 7, title: readOnly ?? "Retirer la sélection de l'archive (Suppr)", action: () => void removeSelected() },
      { id: "test", label: "Tester", glyph: Glyph.check, priority: 6, action: () => void runTest(loc.path, state.opened?.password ?? null) },
      "sep",
      { id: "reveal", label: "Afficher dans l'Explorateur", glyph: Glyph.reveal, priority: 3, action: () => void api.reveal(loc.path) },
      { id: "new", label: "Nouvelle archive", glyph: Glyph.archive, priority: 1, action: () => void pickAndCompress() },
      "sep",
      settingsTool,
    ];
  } else {
    items = [{ id: "back", label: "Retour", glyph: Glyph.back, priority: 1, action: () => void goBack() }];
  }
  ui.toolbar.set(items);
}

async function copyPaths(paths: string[]): Promise<void> {
  await writeText(paths.map((p) => (p.includes(" ") ? `"${p}"` : p)).join("\r\n"));
  toast(paths.length > 1 ? `${paths.length} chemins copiés` : "Chemin copié");
}

// ——— Menus contextuels ———

function stemOf(path: string): string {
  return fmt.baseName(path).replace(ARCHIVE_RE, "");
}

function extractItemsFs(archives: string[]): (MenuItem | "-")[] {
  const one = archives.length === 1;
  return [
    { label: "Extraire ici", hint: "intelligent", glyph: Glyph.extract, action: () => void extractArchives(archives, "here") },
    {
      label: one ? `Extraire dans « ${stemOf(archives[0])} »` : "Extraire chaque archive dans son dossier",
      glyph: Glyph.folder,
      action: () => void extractArchives(archives, "named"),
    },
    { label: "Extraire vers…", glyph: Glyph.reveal, action: () => void extractArchives(archives, "into") },
  ];
}

function extractItemsArchive(): (MenuItem | "-")[] {
  const opened = state.opened;
  if (!opened) return [];
  const hasSel = selectedRows().length > 0;
  return [
    { label: "Extraire ici", hint: "intelligent", glyph: Glyph.extract, action: () => void runExtract("here", false) },
    { label: `Extraire dans « ${stemOf(opened.info.path)} »`, glyph: Glyph.folder, action: () => void runExtract("named", false) },
    { label: "Extraire vers…", glyph: Glyph.reveal, action: () => void runExtract("into", false) },
    "-",
    { label: "Extraire la sélection vers…", glyph: Glyph.check, disabled: !hasSel, action: () => void runExtract("exact", true) },
  ];
}

function fsMenu(rows: Row[]): (MenuItem | "-")[] {
  const loc = state.loc;
  if (loc.kind !== "fs") return [];
  if (!rows.length) {
    return [
      { label: "Actualiser", glyph: "", hint: "F5", action: () => void refresh() },
      { label: "Nouvelle archive…", glyph: Glyph.archive, action: () => void pickAndCompress() },
      "-",
      { label: "Afficher dans l'Explorateur", glyph: Glyph.reveal, action: () => void api.openPath(loc.path) },
      { label: "Copier le chemin du dossier", glyph: "", action: () => void copyPaths([loc.path]) },
    ];
  }
  const paths = rows.map((r) => r.key);
  const one = rows.length === 1 ? rows[0] : null;
  const allArchives = rows.every((r) => r.isArchive);
  const base = fmt.baseName(paths.length === 1 ? (one!.isDir ? one!.key : stemOf(one!.key).replace(/\.[^.]+$/, "") || one!.key) : loc.path.replace(/\\$/, ""));
  const items: (MenuItem | "-")[] = [];
  if (one?.isArchive) {
    items.push({ label: "Ouvrir dans Lynk", glyph: Glyph.archive, hint: "Entrée", action: () => void activate(one) });
    items.push({ label: "Ouvrir avec l'application par défaut", glyph: Glyph.open, action: () => void api.openPath(one.key) });
  } else if (one) {
    items.push({ label: "Ouvrir", glyph: one.isDir ? Glyph.folder : Glyph.open, hint: "Entrée", action: () => void activate(one) });
  }
  if (allArchives) items.push("-", ...extractItemsFs(paths));
  items.push(
    "-",
    { label: `Ajouter à « ${base}.zip »`, glyph: Glyph.archive, action: () => void quickCompress(paths, "zip") },
    { label: `Ajouter à « ${base}.7z »`, glyph: Glyph.archive, action: () => void quickCompress(paths, "seven-z") },
    { label: "Ajouter à une archive…", glyph: Glyph.add, action: () => void compressDialog(paths, loc.path) },
    "-",
    { label: "Afficher dans l'Explorateur", glyph: Glyph.reveal, action: () => void api.reveal(paths[0]) },
    { label: rows.length > 1 ? "Copier les chemins" : "Copier le chemin", glyph: "", action: () => void copyPaths(paths) },
  );
  return items;
}

function archiveMenu(rows: Row[]): (MenuItem | "-")[] {
  if (!rows.length) {
    return [
      { label: "Ajouter des fichiers…", glyph: Glyph.add, disabled: !canModify(), action: () => void pickAndAdd() },
      { label: "Actualiser", glyph: "", hint: "F5", action: () => void refresh() },
    ];
  }
  const one = rows.length === 1 ? rows[0] : null;
  return [
    one ? (one.isDir ? { label: "Ouvrir", glyph: Glyph.folder, action: () => void activate(one) } : { label: "Ouvrir", glyph: Glyph.open, hint: "Entrée", action: () => void activate(one) }) : "-",
    { label: "Extraire vers…", glyph: Glyph.extract, action: () => void runExtract("exact", true) },
    "-",
    { label: "Supprimer de l'archive", glyph: Glyph.delete, hint: "Suppr", disabled: !canModify(), action: () => void removeSelected() },
  ];
}

// ——— Extraction ———

/** Extraction d'archives choisies dans un dossier du disque (une après l'autre). */
async function extractArchives(paths: string[], mode: ExtractMode): Promise<void> {
  let dest: string | null = null;
  if (mode === "into") {
    const picked = await open({ directory: true, title: "Extraire vers", defaultPath: fmt.parentDir(paths[0]) });
    if (typeof picked !== "string") return;
    dest = picked;
  }
  let last: string | null = null;
  let files = 0;
  for (const archive of paths) {
    const report = await extractWithRetry(archive, null, { mode, dest, selection: null });
    if (!report) return;
    last = report.destination;
    files += report.files;
  }
  if (!last) return;
  const where = paths.length > 1 ? `${paths.length} archives` : fmt.baseName(last);
  toast(`${files.toLocaleString("fr-FR")} fichiers extraits → ${where}`, { label: "Ouvrir le dossier", run: () => void api.openPath(last!) });
  if (settings().openFolderAfterExtract) void api.openPath(last);
  if (state.loc.kind === "fs") await refresh();
}

async function extractWithRetry(
  archive: string,
  password: string | null,
  req: { mode: ExtractMode; dest: string | null; selection: Selection | null },
): Promise<ExtractReport | null> {
  for (let attempt = 0; ; attempt++) {
    try {
      const report = await withProgress(`Extraction de ${fmt.baseName(archive)}`, (onProgress) =>
        extract({ archive, ...req, password, overwrite: settings().overwrite }, onProgress),
      );
      if (state.opened?.info.path === archive) state.opened.password = password;
      return report;
    } catch (e) {
      if (isCmdError(e) && e.kind === "cancelled") return null;
      if (isCmdError(e) && e.kind === "password") {
        password = await askPassword(fmt.baseName(archive), attempt > 0 || password !== null);
        if (password === null) return null;
        continue;
      }
      await alertError("Échec de l'extraction", errorMessage(e));
      return null;
    }
  }
}

/** Extraction depuis l'archive ouverte (tout, ou la sélection). */
async function runExtract(mode: ExtractMode, onlySelection: boolean): Promise<void> {
  const opened = state.opened;
  const loc = state.loc;
  if (!opened || loc.kind !== "archive") return;
  let dest: string | null = null;
  if (mode === "into" || mode === "exact") {
    const picked = await open({ directory: true, title: "Extraire vers", defaultPath: fmt.parentDir(opened.info.path) });
    if (typeof picked !== "string") return;
    dest = picked;
  }
  const items = selectedRows().map((r) => r.key);
  const selection = onlySelection && items.length ? { base: state.filter ? "" : loc.inner, items } : null;
  const report = await extractWithRetry(opened.info.path, opened.password, { mode, dest, selection });
  if (!report) return;
  const skipped = report.skipped.length ? ` (${report.skipped.length} ignorés)` : "";
  toast(`${report.files.toLocaleString("fr-FR")} fichiers extraits dans ${fmt.baseName(report.destination)}${skipped}`, {
    label: "Ouvrir le dossier",
    run: () => void api.openPath(report.destination),
  });
  if (settings().openFolderAfterExtract) void api.openPath(report.destination);
}

async function openEntry(entry: string): Promise<void> {
  const opened = state.opened;
  if (!opened) return;
  try {
    await api.openEntry(opened.info.path, entry, opened.password);
  } catch (e) {
    if (isCmdError(e) && e.kind === "password") {
      const pw = await askPassword(opened.info.name, opened.password !== null);
      if (pw === null) return;
      opened.password = pw;
      return openEntry(entry);
    }
    await alertError("Impossible d'ouvrir le fichier", errorMessage(e));
  }
}

async function runTest(archive: string, password: string | null): Promise<void> {
  for (let attempt = 0; ; attempt++) {
    try {
      const report = await withProgress(`Test de ${fmt.baseName(archive)}`, (onProgress) => testArchive({ archive, password }, onProgress));
      const ok = report.errors.length === 0;
      const n = report.errors.length;
      await modal<void>((close) => ({
        title: ok ? "Archive intacte" : "Archive endommagée",
        body: ok
          ? h("p", {}, icon(Glyph.check, "ok"), ` ${report.files.toLocaleString("fr-FR")} fichiers vérifiés (${fmt.size(report.bytes)}), aucune erreur.`)
          : h(
              "div",
              { class: "stack" },
              h("p", {}, icon(Glyph.error, "err"), ` ${n} erreur${n > 1 ? "s" : ""} détectée${n > 1 ? "s" : ""}.`),
              h("pre", { class: "error-list" }, report.errors.slice(0, 50).join("\n")),
            ),
        actions: [button("Fermer", { variant: "accent", onclick: () => close() })],
      }), undefined);
      if (state.opened?.info.path === archive) state.opened.password = password;
      return;
    } catch (e) {
      if (isCmdError(e) && e.kind === "cancelled") return;
      if (isCmdError(e) && e.kind === "password") {
        password = await askPassword(fmt.baseName(archive), attempt > 0 || password !== null);
        if (password === null) return;
        continue;
      }
      await alertError("Test impossible", errorMessage(e));
      return;
    }
  }
}

// ——— Modification de l'archive ouverte ———

async function runUpdate(title: string, req: Omit<UpdateRequest, "archive" | "password">): Promise<boolean> {
  const opened = state.opened;
  if (!opened) return false;
  for (let attempt = 0; ; attempt++) {
    try {
      await withProgress(title, (onProgress) => update({ ...req, archive: opened.info.path, password: opened.password, level: settings().defaultLevel }, onProgress));
      await reloadArchive();
      await go(state.loc, { push: false });
      return true;
    } catch (e) {
      if (isCmdError(e) && e.kind === "cancelled") return false;
      if (isCmdError(e) && e.kind === "password") {
        const pw = await askPassword(opened.info.name, attempt > 0 || opened.password !== null);
        if (pw === null) return false;
        opened.password = pw;
        continue;
      }
      await alertError("Échec de la modification", errorMessage(e));
      return false;
    }
  }
}

async function addToArchive(paths: string[]): Promise<void> {
  const opened = state.opened;
  const loc = state.loc;
  if (!opened || loc.kind !== "archive") return;
  // Archive chiffrée : les ajouts le seront aussi, avec le même mot de passe.
  if (opened.root.encrypted && opened.password === null) {
    const pw = await askPassword(opened.info.name, false);
    if (pw === null) return;
    opened.password = pw;
  }
  const label = paths.length === 1 ? fmt.baseName(paths[0]) : `${paths.length} éléments`;
  if (await runUpdate(`Ajout à ${opened.info.name}`, { base: loc.inner, add: paths })) {
    toast(`${label} ajouté${paths.length > 1 ? "s" : ""} à ${opened.info.name}`);
  }
}

async function pickAndAdd(): Promise<void> {
  const picked = await open({ multiple: true, title: "Fichiers à ajouter" });
  if (Array.isArray(picked) && picked.length) await addToArchive(picked);
}

async function removeSelected(): Promise<void> {
  const opened = state.opened;
  const loc = state.loc;
  const items = selectedRows().map((r) => r.key);
  if (!opened || loc.kind !== "archive" || !items.length || !canModify()) return;
  const ok = await modal<boolean>((close) => ({
    title: "Supprimer de l'archive ?",
    body: h(
      "p",
      {},
      items.length === 1 ? `« ${items[0].split("/").pop()} » sera retiré de ${opened.info.name}.` : `${items.length} éléments seront retirés de ${opened.info.name}.`,
      " Les fichiers sur le disque ne sont pas touchés.",
    ),
    actions: [
      button("Supprimer", { variant: "accent", onclick: () => close(true) }),
      button("Annuler", { variant: "standard", onclick: () => close(false) }),
    ],
  }), false);
  if (ok) await runUpdate(`Suppression dans ${opened.info.name}`, { base: loc.inner, remove: items });
}

// ——— Création ———

async function pickAndCompress(): Promise<void> {
  const picked = await open({ multiple: true, title: "Fichiers à archiver" });
  if (Array.isArray(picked) && picked.length) await compressDialog(picked);
}

function formatExt(format: FormatId): string {
  return state.formats.find((f) => f.id === format)?.extension ?? "zip";
}

/** « Ajouter à « x.zip » » : création directe avec les réglages par défaut. */
async function quickCompress(sources: string[], format: FormatId): Promise<void> {
  const dest = await api.suggestArchivePath(sources, format);
  await runCreate(sources, { dest, format, level: settings().defaultLevel, password: null, volumeSize: null });
}

async function runCreate(
  sources: string[],
  req: { dest: string; format: FormatId; level: number; password: string | null; volumeSize: number | null },
): Promise<void> {
  try {
    const created = await withProgress(`Création de ${fmt.baseName(req.dest)}`, (onProgress) => create({ sources, ...req }, onProgress));
    const first = created.paths[0];
    const what = created.paths.length > 1 ? `${fmt.baseName(req.dest)} créée en ${created.paths.length} volumes` : `${fmt.baseName(first)} créée`;
    toast(`${what} (${fmt.size(created.size)})`, { label: "Afficher", run: () => void api.reveal(first) });
    // Si l'archive arrive dans le dossier affiché, on l'y montre sélectionnée.
    if (state.loc.kind === "fs" && fmt.parentDir(first).toLowerCase() === state.loc.path.replace(/\\$/, "").toLowerCase()) {
      await go(state.loc, { push: false, select: [first] });
    }
  } catch (e) {
    if (isCmdError(e) && e.kind === "cancelled") return;
    await alertError("Échec de la création", errorMessage(e));
  }
}

async function compressDialog(sources: string[], targetDir?: string): Promise<void> {
  const creatable = state.formats.filter((f) => f.canCreate);
  const s = settings();
  let format: FormatId = creatable.some((f) => f.id === s.defaultFormat) ? s.defaultFormat : "zip";
  let destEdited = false;
  const suggest = async (f: FormatId) => {
    const path = await api.suggestArchivePath(sources, f);
    return targetDir ? `${targetDir.replace(/\\$/, "")}\\${fmt.baseName(path)}` : path;
  };

  const dest = h("input", { class: "field grow", value: await suggest(format) });
  dest.addEventListener("input", () => (destEdited = true));
  const formatSel = h("select", { class: "field" }, ...creatable.map((f) => h("option", { value: f.id, selected: f.id === format }, f.label)));
  const level = h("input", { type: "range", min: 0, max: 9, value: s.defaultLevel, class: "slider" });
  const levelLabel = h("span", { class: "muted level-label" }, LEVELS[s.defaultLevel]);
  level.addEventListener("input", () => (levelLabel.textContent = LEVELS[Number(level.value)]));
  const password = h("input", { type: "password", class: "field grow", placeholder: "Aucun" });
  const pwHint = h("span", { class: "muted small" }, "AES-256. En 7z, les noms de fichiers sont aussi chiffrés.");
  const syncPassword = () => {
    const info = creatable.find((f) => f.id === format);
    password.disabled = !info?.canEncrypt;
    pwHint.textContent = info?.canEncrypt ? "AES-256. En 7z, les noms de fichiers sont aussi chiffrés." : "Ce format ne gère pas le chiffrement.";
  };
  syncPassword();

  formatSel.addEventListener("change", async () => {
    format = formatSel.value as FormatId;
    syncPassword();
    // Nom choisi à la main : seule l'extension suit le format.
    dest.value = destEdited ? `${dest.value.replace(ARCHIVE_RE, "")}.${formatExt(format)}` : await suggest(format);
  });

  const browse = button("Parcourir…", {
    variant: "standard",
    onclick: async () => {
      const e = formatExt(format);
      const picked = await save({ defaultPath: dest.value, filters: [{ name: "Archive", extensions: [e.split(".").pop() ?? e] }] });
      if (picked) {
        dest.value = picked;
        destEdited = true;
      }
    },
  });

  // Volumes : tailles usuelles, en Mio comme 7-Zip et WinRAR.
  const MiB = 1024 * 1024;
  const volumeSel = h(
    "select",
    { class: "field" },
    h("option", { value: "0" }, "Pas de découpage"),
    h("option", { value: String(25 * MiB) }, "25 Mo (pièce jointe)"),
    h("option", { value: String(100 * MiB) }, "100 Mo"),
    h("option", { value: String(700 * MiB) }, "700 Mo (CD)"),
    h("option", { value: String(4095 * MiB) }, "4 Go (clé USB FAT32)"),
    h("option", { value: "custom" }, "Autre taille…"),
  );
  const volumeCustom = h("input", { type: "number", min: 1, value: 50, class: "field volume-size", hidden: true });
  const volumeUnit = h("span", { class: "muted", hidden: true }, "Mo");
  volumeSel.addEventListener("change", () => {
    volumeCustom.hidden = volumeUnit.hidden = volumeSel.value !== "custom";
  });
  const volumeSize = (): number | null =>
    volumeSel.value === "custom" ? Math.max(1, Number(volumeCustom.value) || 0) * MiB : Number(volumeSel.value) || null;

  const label = sources.length === 1 ? fmt.baseName(sources[0]) : `${sources.length} éléments`;
  const ok = await modal<boolean>((close) => ({
    title: "Nouvelle archive",
    className: "create-dialog",
    body: h(
      "div",
      { class: "form" },
      h("p", { class: "muted" }, icon(Glyph.file), ` ${label}`),
      h("label", {}, "Archive"),
      h("div", { class: "row" }, dest, browse),
      h("label", {}, "Format"),
      formatSel,
      h("label", {}, "Compression"),
      h("div", { class: "row" }, level, levelLabel),
      h("label", {}, "Mot de passe"),
      h("div", { class: "stack-tight" }, password, pwHint),
      h("label", {}, "Volumes"),
      h("div", { class: "row" }, volumeSel, volumeCustom, volumeUnit),
    ),
    actions: [button("Créer", { variant: "accent", onclick: () => close(true) }), button("Annuler", { variant: "standard", onclick: () => close(false) })],
  }), false);
  if (!ok) return;
  await runCreate(sources, {
    dest: dest.value,
    format,
    level: Number(level.value),
    password: password.disabled ? null : password.value || null,
    volumeSize: volumeSize(),
  });
}

// ——— Glisser-déposer ———

async function dragOut(): Promise<void> {
  const loc = state.loc;
  const rows = selectedRows();
  if (!rows.length || state.draggingOut) return;
  state.draggingOut = true;
  document.body.classList.add("drag-out");
  try {
    if (loc.kind === "fs") await api.dragPaths(rows.map((r) => r.key));
    else if (loc.kind === "archive" && state.opened) await api.dragOut(state.opened.info.path, rows.map((r) => r.key), state.opened.password);
  } catch (e) {
    const opened = state.opened;
    if (isCmdError(e) && e.kind === "password" && opened) {
      const pw = await askPassword(opened.info.name, opened.password !== null);
      if (pw !== null) {
        opened.password = pw;
        toast("Mot de passe accepté : glissez à nouveau les fichiers.");
      }
    } else {
      await alertError("Glisser-déposer impossible", errorMessage(e));
    }
  } finally {
    state.draggingOut = false;
    document.body.classList.remove("drag-out");
  }
}

async function listenDragDrop(): Promise<void> {
  await getCurrentWebview().onDragDropEvent(async (event) => {
    const p = event.payload;
    if (p.type === "enter") {
      document.body.classList.add("dragging");
      const text = ui.drop.querySelector(".drop-text");
      if (text) text.textContent = canModify() ? `Déposez pour ajouter à ${state.opened?.info.name}` : "Déposez pour ouvrir ou archiver";
    } else if (p.type === "leave") document.body.classList.remove("dragging");
    else if (p.type === "drop") {
      document.body.classList.remove("dragging");
      if (state.draggingOut || !p.paths.length) return;
      await onDrop(p.paths);
    }
  });
}

async function onDrop(paths: string[]): Promise<void> {
  const single = paths.length === 1 && ARCHIVE_RE.test(paths[0]);
  if (!canModify()) {
    if (single) await openPathOrArchive(paths[0]);
    else await compressDialog(paths, state.loc.kind === "fs" ? state.loc.path : undefined);
    return;
  }
  // Une archive est ouverte : comme WinRAR, on propose d'y ajouter les fichiers.
  const name = state.opened!.info.name;
  const loc = state.loc as Extract<Loc, { kind: "archive" }>;
  const where = loc.inner ? ` (dans ${loc.inner.split("/").pop()})` : "";
  const label = paths.length === 1 ? `« ${fmt.baseName(paths[0])} »` : `${paths.length} éléments`;
  const choice = await modal<"add" | "open" | "new" | null>((close) => ({
    title: "Que faire de ces fichiers ?",
    body: h("p", {}, `${label} déposé${paths.length > 1 ? "s" : ""}.`),
    actions: [
      button(`Ajouter à ${name}${where}`, { variant: "accent", onclick: () => close("add") }),
      single ? button("Ouvrir", { variant: "standard", onclick: () => close("open") }) : null,
      button("Nouvelle archive", { variant: "standard", onclick: () => close("new") }),
      button("Annuler", { variant: "standard", onclick: () => close(null) }),
    ].filter((b): b is HTMLButtonElement => b !== null),
  }), null);
  if (choice === "add") await addToArchive(paths);
  else if (choice === "open") await openPathOrArchive(paths[0]);
  else if (choice === "new") await compressDialog(paths);
}

// ——— Clavier et barre d'état ———

function onGlobalKey(e: KeyboardEvent): void {
  const k = e.key.toLowerCase();
  const typing = e.target instanceof Element && e.target.closest("input, textarea, select");
  if (e.altKey && e.key === "ArrowLeft") (e.preventDefault(), void goBack());
  else if (e.altKey && e.key === "ArrowRight") (e.preventDefault(), void goForward());
  else if (e.altKey && e.key === "ArrowUp") (e.preventDefault(), void goUp());
  else if (e.key === "F5") (e.preventDefault(), void refresh());
  else if ((e.ctrlKey && k === "l") || (e.altKey && k === "d")) (e.preventDefault(), editAddress());
  else if (e.ctrlKey && k === "f") (e.preventDefault(), ui.search.focus());
  else if (e.ctrlKey && k === "o") (e.preventDefault(), void pickArchive());
  else if (e.ctrlKey && k === "n") {
    e.preventDefault();
    const sel = selectedRows();
    if (state.loc.kind === "fs" && sel.length) void compressDialog(sel.map((r) => r.key), state.loc.path);
    else void pickAndCompress();
  } else if (e.ctrlKey && e.key === ",") (e.preventDefault(), void go({ kind: "settings" }));
  else if (e.key === "Escape" && !typing && state.sidebarOpen) toggleSidebar();
}

function renderStatus(): void {
  const loc = state.loc;
  const sel = selectedRows();
  const selText = (() => {
    if (!sel.length) return "";
    const size = sel.reduce((s, r) => s + (r.isDir ? 0 : (r.size ?? 0)), 0);
    return `${sel.length} sélectionné${sel.length > 1 ? "s" : ""}${size ? ` · ${fmt.size(size)}` : ""}`;
  })();
  let left = "";
  if (loc.kind === "fs") {
    const n = ui.fsList.all().length;
    left = `${n.toLocaleString("fr-FR")} élément${n > 1 ? "s" : ""}`;
  } else if (loc.kind === "archive" && state.opened) {
    const o = state.opened;
    const total = o.root.size;
    const parts = [o.info.formatLabel, `${countFiles(o.root).toLocaleString("fr-FR")} fichiers`, `${fmt.size(total)} → ${fmt.size(o.info.size)}`];
    if (total > 0) parts.push(`taux ${fmt.ratio(total, o.info.size)}`);
    if (!canModify()) parts.push("lecture seule");
    left = parts.join(" · ");
  } else if (loc.kind === "home") {
    left = `${state.places.drives.length} lecteur${state.places.drives.length > 1 ? "s" : ""}`;
  }
  ui.status.replaceChildren(h("span", { class: "muted ellipsis" }, left), h("span", { class: "ellipsis" }, selText));
}
