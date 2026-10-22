import { getCurrentWindow } from "@tauri-apps/api/window";

import { api } from "./api";
import { startExplorer } from "./explorer";
import { applyAppearance, loadSettings, setSystemAccent } from "./settings";
import { runTask } from "./task";

async function main(): Promise<void> {
  // Le menu contextuel du navigateur (« Actualiser », « Inspecter ») n'a rien à faire ici.
  document.addEventListener("contextmenu", (e) => {
    const target = e.target as HTMLElement;
    if (!target.closest("input, textarea")) e.preventDefault();
  });

  const [accent, s, launch] = await Promise.all([api.accent().catch(() => null), loadSettings(), api.launch()]);
  setSystemAccent(accent?.base ?? null);
  await applyAppearance(s);
  // Le registre (lu par le menu contextuel) suit toujours les réglages.
  void api.setContextMenuHidden(s.menuHidden).catch(() => {});

  const root = document.getElementById("app")!;
  const isTask = launch.files.length > 0 && ["extract-here", "extract-named", "extract-to", "compress"].includes(launch.action);

  if (isTask) {
    // La fenêtre de tâche ne s'affiche qu'une fois le dossier choisi (« Extraire vers… »).
    await runTask(root, launch);
    return;
  }
  try {
    await startExplorer(root, {
      open: launch.action === "open" ? launch.files[0] : undefined,
      compress: launch.action === "compress-dialog" ? launch.files : undefined,
    });
  } finally {
    // Jamais de fenêtre invisible coincée en arrière-plan, même en cas d'erreur.
    await getCurrentWindow().show();
  }
}

void main();
