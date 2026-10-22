// Nouvelles versions : lecture publique de la dernière version publiée sur GitHub.
// Rien n'est envoyé, rien n'est téléchargé : WinSAR prévient, l'utilisateur décide.

import { getVersion } from "@tauri-apps/api/app";

import { settings, updateSettings } from "./settings";

export const REPO = "moha528/winsar";
export const REPO_URL = `https://github.com/${REPO}`;

const DAY = 24 * 3600 * 1000;

export interface Release {
  version: string;
  /** Page de la version sur GitHub (notes et installeur). */
  url: string;
  notes: string;
}

export interface UpdateStatus {
  current: string;
  /** `null` : aucune version publiée pour l'instant. */
  latest: Release | null;
  available: boolean;
}

/** `1.10.0` > `1.9.2`. Une préversion (`1.0.0-beta`) passe avant la version finale. */
export function isNewer(candidate: string, current: string): boolean {
  const parse = (v: string) => {
    const [core, pre] = v.trim().replace(/^v/i, "").split("-", 2);
    return { nums: core.split(".").map((n) => Number.parseInt(n, 10) || 0), pre: pre ?? null };
  };
  const a = parse(candidate);
  const b = parse(current);
  for (let i = 0; i < Math.max(a.nums.length, b.nums.length); i++) {
    const d = (a.nums[i] ?? 0) - (b.nums[i] ?? 0);
    if (d !== 0) return d > 0;
  }
  if (a.pre === null || b.pre === null) return a.pre === null && b.pre !== null;
  return a.pre > b.pre;
}

async function latestRelease(): Promise<Release | null> {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`GitHub a répondu ${res.status}`);
  const r = (await res.json()) as { tag_name?: string; html_url?: string; body?: string };
  const url = typeof r.html_url === "string" && r.html_url.startsWith(`${REPO_URL}/`) ? r.html_url : `${REPO_URL}/releases/latest`;
  return { version: String(r.tag_name ?? "").replace(/^v/i, ""), url, notes: r.body ?? "" };
}

export async function checkForUpdate(): Promise<UpdateStatus> {
  const [current, latest] = await Promise.all([getVersion(), latestRelease()]);
  updateSettings({ lastUpdateCheck: Date.now() });
  return { current, latest, available: latest !== null && isNewer(latest.version, current) };
}

/** Au démarrage, une fois par jour au plus et en silence : une erreur réseau n'a rien d'alarmant. */
export async function backgroundCheck(onAvailable: (release: Release) => void): Promise<void> {
  const s = settings();
  if (import.meta.env.DEV || !s.checkUpdates || Date.now() - s.lastUpdateCheck < DAY) return;
  try {
    const status = await checkForUpdate();
    if (status.available && status.latest && status.latest.version !== s.skippedVersion) onAvailable(status.latest);
  } catch {
    // Hors ligne, ou GitHub injoignable : on réessaiera au prochain démarrage.
  }
}
