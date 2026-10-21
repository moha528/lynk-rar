// Réglages de Lynk : schéma, valeurs par défaut, chargement, sauvegarde, application.

import { Effect, getCurrentWindow } from "@tauri-apps/api/window";

import { api, type FormatId, type Overwrite } from "./api";

export type Theme = "system" | "light" | "dark";
export type StartPlace = "home" | "downloads" | "last" | "custom";

export interface Settings {
  theme: Theme;
  /** Effet Mica de Windows 11 derrière la fenêtre. */
  mica: boolean;
  start: StartPlace;
  startPath: string;
  /** Dernier emplacement visité, pour `start: "last"`. */
  lastPath: string;
  showHidden: boolean;
  sidebar: boolean;
  defaultFormat: FormatId;
  defaultLevel: number;
  overwrite: Overwrite;
  openFolderAfterExtract: boolean;
  notifications: boolean;
  /** Entrées masquées du menu contextuel (bit n = n-ième entrée, voir MENU_ENTRIES). */
  menuHidden: number;
  /** Largeur des colonnes par vue (`fs`, `archive`) puis par colonne (`name`, `size`…). */
  columns: Record<string, Record<string, number>>;
  /** Rechercher une nouvelle version sur GitHub au démarrage (une fois par jour). */
  checkUpdates: boolean;
  /** Dernière recherche de mise à jour, en millisecondes Unix. */
  lastUpdateCheck: number;
  /** Version dont l'utilisateur ne veut plus entendre parler. */
  skippedVersion: string;
}

export const DEFAULTS: Settings = {
  theme: "system",
  mica: true,
  start: "home",
  startPath: "",
  lastPath: "",
  showHidden: false,
  sidebar: true,
  defaultFormat: "zip",
  defaultLevel: 6,
  overwrite: "rename",
  openFolderAfterExtract: false,
  notifications: true,
  menuHidden: 0,
  columns: {},
  checkUpdates: true,
  lastUpdateCheck: 0,
  skippedVersion: "",
};

/** Même ordre que `Verb::ALL` dans crates/lynk-shell (le séparateur occupe le bit 4). */
export const MENU_ENTRIES: { bit: number; label: string }[] = [
  { bit: 0, label: "Extraire ici" },
  { bit: 1, label: "Extraire dans « nom\\ »" },
  { bit: 2, label: "Extraire vers…" },
  { bit: 3, label: "Ouvrir avec Lynk" },
  { bit: 5, label: "Ajouter à « nom.zip »" },
  { bit: 6, label: "Ajouter à « nom.7z »" },
  { bit: 7, label: "Ajouter à une archive…" },
];

let current: Settings = { ...DEFAULTS };
let saveTimer: number | undefined;
const listeners = new Set<(s: Settings) => void>();

export async function loadSettings(): Promise<Settings> {
  const raw = await api.settingsLoad().catch(() => null);
  current = { ...DEFAULTS, ...(raw && typeof raw === "object" ? (raw as Partial<Settings>) : {}) };
  return current;
}

export function settings(): Settings {
  return current;
}

/** Modifie et enregistre (regroupé : un glissement de colonne ne fait qu'une écriture). */
export function updateSettings(patch: Partial<Settings>): void {
  current = { ...current, ...patch };
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void api.settingsSave(current), 400);
  listeners.forEach((l) => l(current));
}

export function onSettingsChange(listener: (s: Settings) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Thème et Mica : le thème pilote à la fois la fenêtre native et les jetons CSS. */
export async function applyAppearance(s: Settings = current): Promise<void> {
  const root = document.documentElement;
  if (s.theme === "system") delete root.dataset.theme;
  else root.dataset.theme = s.theme;
  root.classList.toggle("no-mica", !s.mica);
  const win = getCurrentWindow();
  await win.setTheme(s.theme === "system" ? null : s.theme).catch(() => {});
  if (s.mica) await win.setEffects({ effects: [Effect.Mica] }).catch(() => {});
  else await win.clearEffects().catch(() => {});
}
