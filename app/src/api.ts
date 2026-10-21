import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type FormatId =
  | "zip"
  | "seven-z"
  | "rar"
  | "tar"
  | "tar-gz"
  | "tar-bz2"
  | "tar-xz"
  | "tar-zst";

export interface FormatInfo {
  id: FormatId;
  label: string;
  extension: string;
  canCreate: boolean;
  canEncrypt: boolean;
}

export interface Entry {
  path: string;
  size: number;
  packedSize: number | null;
  isDir: boolean;
  modified: number | null;
  encrypted: boolean;
}

export interface ArchiveInfo {
  path: string;
  name: string;
  format: FormatId;
  formatLabel: string;
  /** Taille sur le disque, tous volumes compris. */
  size: number;
  /** Nombre de volumes `.001`, `.002`… (1 pour une archive d'un seul tenant). */
  volumes: number;
  entries: Entry[];
}

export type Action =
  | "open"
  | "extract-here"
  | "extract-named"
  | "extract-to"
  | "compress"
  | "compress-dialog";

export interface Launch {
  action: Action;
  files: string[];
  format: FormatId | null;
}

export interface Selection {
  base: string;
  items: string[];
}

export type ExtractMode = "here" | "named" | "into" | "exact";
export type Overwrite = "skip" | "replace" | "rename";

export interface ExtractRequest {
  archive: string;
  mode: ExtractMode;
  dest?: string | null;
  password?: string | null;
  overwrite?: Overwrite;
  selection?: Selection | null;
}

export interface ExtractReport {
  destination: string;
  files: number;
  dirs: number;
  bytes: number;
  skipped: string[];
}

export interface CreateRequest {
  sources: string[];
  dest: string;
  format: FormatId;
  level: number;
  password?: string | null;
  /** Découpe en volumes `.001`, `.002`… de cette taille, en octets. */
  volumeSize?: number | null;
}

export interface Created {
  /** L'archive, ou ses volumes dans l'ordre. */
  paths: string[];
  size: number;
}

export interface TestReport {
  files: number;
  bytes: number;
  errors: string[];
}

export interface FsEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified: number | null;
  hidden: boolean;
}

export interface Place {
  name: string;
  path: string;
  icon: string | null;
  total: number | null;
  free: number | null;
}

export interface Places {
  folders: Place[];
  drives: Place[];
  home: string;
}

export interface FileTypeStatus {
  registered: boolean;
  default: boolean;
  current: string | null;
}

export interface RecentItem {
  path: string;
  name: string;
  size: number;
  opened: number;
}

export interface CmdError {
  kind: "password" | "cancelled" | "error";
  message: string;
}

export interface ProgressEvent {
  job: number;
  done: number;
  total: number;
  current: string;
}

export function isCmdError(e: unknown): e is CmdError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isCmdError(e)) return e.message;
  return e instanceof Error ? e.message : String(e);
}

export const api = {
  launch: () => invoke<Launch>("launch_info"),
  formats: () => invoke<FormatInfo[]>("formats"),
  open: (path: string, password?: string | null) =>
    invoke<ArchiveInfo>("open_archive", { path, password: password ?? null }),
  /** Nom libre pour une nouvelle archive, à côté des sources ou dans `dir`. */
  suggestArchivePath: (sources: string[], format: FormatId, dir?: string | null) =>
    invoke<string>("suggest_archive_path", { sources, format, dir: dir ?? null }),
  openEntry: (archive: string, entry: string, password?: string | null) =>
    invoke<void>("open_entry", { archive, entry, password: password ?? null }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  /** Feuille de propriétés de Windows (comme Alt+Entrée dans l'Explorateur). */
  properties: (path: string) => invoke<void>("properties", { path }),
  /** Aperçu rapide d'un fichier du disque. */
  peek: (path: string) => invoke<Peek>("peek", { path }),
  accent: () => invoke<{ light: string; dark: string } | null>("system_accent"),
  /** Icônes système par type : `.pdf`, `""` (sans extension), `/` (dossier). */
  fileIcons: (keys: string[]) => invoke<Record<string, string>>("file_icons", { keys }),
  /** Glisser-déposer natif vers l'Explorateur ; `true` si les fichiers ont été déposés. */
  dragOut: (archive: string, items: string[], password?: string | null) =>
    invoke<boolean>("drag_out", { archive, items, password: password ?? null }),
  recentList: () => invoke<RecentItem[]>("recent_list"),
  recentAdd: (path: string) => invoke<void>("recent_add", { path }),
  recentRemove: (path: string) => invoke<void>("recent_remove", { path }),
  /** Notification Windows ; un clic ouvre `folder`. */
  notify: (title: string, body: string, folder?: string | null) =>
    invoke<void>("notify", { title, body, folder: folder ?? null }),

  listDir: (path: string, showHidden: boolean) => invoke<FsEntry[]>("list_dir", { path, showHidden }),
  places: () => invoke<Places>("places"),
  /** Icônes propres à des fichiers précis (programmes, raccourcis). Clé : chemin. */
  pathIcons: (paths: string[]) => invoke<Record<string, string>>("path_icons", { paths }),
  dragPaths: (paths: string[]) => invoke<boolean>("drag_paths", { paths }),
  pathKind: (path: string) => invoke<"dir" | "file" | "missing">("path_kind", { path }),

  settingsLoad: () => invoke<unknown>("settings_load"),
  settingsSave: (value: unknown) => invoke<void>("settings_save", { value }),
  setContextMenuHidden: (mask: number) => invoke<void>("set_context_menu_hidden", { mask }),
  fileTypesStatus: (exts: string[]) => invoke<Record<string, FileTypeStatus>>("file_types_status", { exts }),
  registerFileTypes: (exts: string[], enable: boolean) => invoke<void>("register_file_types", { exts, enable }),
  openDefaultApps: () => invoke<void>("open_default_apps"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export interface Job<T> {
  done: Promise<T>;
  cancel(): void;
}

let nextJob = 1;

/** Lance une opération longue côté Rust et relaie son avancement. */
function startJob<T>(
  command: "extract" | "create" | "update" | "test_archive" | "convert" | "materialize_entries" | "hash_files",
  req: object,
  onProgress: (p: ProgressEvent) => void,
): Job<T> {
  const job = nextJob++;
  const listening = listen<ProgressEvent>("job-progress", (e) => {
    if (e.payload.job === job) onProgress(e.payload);
  });
  const done = (async () => {
    const unlisten = await listening;
    try {
      return await invoke<T>(command, { req: { ...req, job } });
    } finally {
      unlisten();
    }
  })();
  return { done, cancel: () => void invoke("cancel", { job }) };
}

export const extract = (req: ExtractRequest, onProgress: (p: ProgressEvent) => void) =>
  startJob<ExtractReport>("extract", req, onProgress);

export const create = (req: CreateRequest, onProgress: (p: ProgressEvent) => void) =>
  startJob<Created>("create", req, onProgress);

export interface UpdateRequest {
  archive: string;
  /** Dossier de l'archive où déposer les ajouts ("" : racine). */
  base: string;
  add?: string[];
  remove?: string[];
  password?: string | null;
  level?: number;
}

export const update = (req: UpdateRequest, onProgress: (p: ProgressEvent) => void) =>
  startJob<void>("update", req, onProgress);

export const testArchive = (
  req: { archive: string; password?: string | null },
  onProgress: (p: ProgressEvent) => void,
) => startJob<TestReport>("test_archive", req, onProgress);

export interface ConvertRequest {
  archive: string;
  dest: string;
  /** Mot de passe de l'archive d'origine. */
  password?: string | null;
  format: FormatId;
  level: number;
  /** Mot de passe de la nouvelle archive. */
  newPassword?: string | null;
  volumeSize?: number | null;
}

export const convert = (req: ConvertRequest, onProgress: (p: ProgressEvent) => void) =>
  startJob<Created>("convert", req, onProgress);

/** Copie temporaire d'éléments de l'archive ; rend leurs chemins sur le disque. */
export const materialize = (
  req: { archive: string; items: string[]; password?: string | null },
  onProgress: (p: ProgressEvent) => void,
) => startJob<string[]>("materialize_entries", req, onProgress);

export type Peek =
  | { kind: "image"; url: string }
  | { kind: "text"; text: string; truncated: boolean }
  | { kind: "other" };

export interface FileHash {
  path: string;
  size: number;
  crc32: string;
  sha256: string;
}

export const hashFiles = (req: { paths: string[] }, onProgress: (p: ProgressEvent) => void) =>
  startJob<FileHash[]>("hash_files", req, onProgress);

/** Extensions reconnues comme archives (ouverture par glisser-déposer). */
export const ARCHIVE_RE = /\.(zip|jar|7z|rar|tar|tgz|tbz2?|txz|tzst|tar\.(gz|bz2|xz|zst))(\.\d{3})?$/i;
