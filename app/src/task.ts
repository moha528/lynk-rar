// Mode « tâche » : lancé par le menu contextuel de l'Explorateur, une petite
// fenêtre de progression qui se ferme d'elle-même quand tout s'est bien passé.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";

import {
  api,
  create,
  errorMessage,
  extract,
  isCmdError,
  type ExtractMode,
  type ExtractReport,
  type Job,
  type Launch,
  type ProgressEvent,
} from "./api";
import * as fmt from "./format";
import { settings } from "./settings";
import { button, Glyph, h, icon, ProgressView } from "./ui";

class Cancelled extends Error {}

export async function runTask(root: HTMLElement, launch: Launch): Promise<void> {
  document.body.classList.add("task");
  const title = h("h2", {});
  const progress = new ProgressView(title);
  let job: Job<unknown> | null = null;
  let cancelled = false;
  const cancelBtn = button("Annuler", {
    variant: "standard",
    onclick: () => {
      cancelled = true;
      job?.cancel();
    },
  });
  const card = h("div", { class: "task-card" }, progress.root, h("div", { class: "task-actions" }, cancelBtn));
  root.append(card);
  const win = getCurrentWindow();

  const track = <T>(start: (onProgress: (p: ProgressEvent) => void) => Job<T>): Promise<T> => {
    if (cancelled) throw new Cancelled();
    progress.reset();
    const j = start((p) => progress.update(p));
    job = j;
    return j.done;
  };

  try {
    if (launch.action === "compress") {
      const format = launch.format ?? settings().defaultFormat;
      const dest = await api.suggestArchivePath(launch.files, format);
      setTitle(`Compression dans ${fmt.baseName(dest)}`);
      await win.show();
      const created = await track((onProgress) => create({ sources: launch.files, dest, format, level: settings().defaultLevel }, onProgress));
      if (settings().notifications) await api.notify("Archive créée", `${fmt.baseName(created.paths[0])} · ${fmt.size(created.size)}`, fmt.parentDir(created.paths[0]));
    } else {
      const mode: ExtractMode = launch.action === "extract-named" ? "named" : launch.action === "extract-to" ? "into" : "here";
      let dest: string | null = null;
      if (mode === "into") {
        const picked = await open({ directory: true, title: "Extraire vers", defaultPath: fmt.parentDir(launch.files[0]) });
        if (typeof picked !== "string") return void (await win.close());
        dest = picked;
      }
      await win.show();
      const n = launch.files.length;
      const reports: ExtractReport[] = [];
      for (const [i, archive] of launch.files.entries()) {
        setTitle(n > 1 ? `Extraction ${i + 1}/${n} : ${fmt.baseName(archive)}` : `Extraction de ${fmt.baseName(archive)}`);
        reports.push(await extractWithPassword(archive, mode, dest, track));
      }
      const files = reports.reduce((sum, r) => sum + r.files, 0);
      const last = reports[reports.length - 1];
      const where = n > 1 ? `${n} archives` : fmt.baseName(last.destination);
      if (settings().openFolderAfterExtract) void api.openPath(last.destination);
      if (settings().notifications) await api.notify("Extraction terminée", `${files.toLocaleString("fr-FR")} fichiers → ${where}`, last.destination);
    }
    setTitle("Terminé");
    setTimeout(() => void win.close(), 350);
  } catch (e) {
    if (e instanceof Cancelled || (isCmdError(e) && e.kind === "cancelled")) return void (await win.close());
    await win.show();
    card.replaceChildren(
      h("div", { class: "task-error" }, icon(Glyph.error, "err"), h("div", {}, h("h2", {}, title.textContent ?? "Erreur"), h("p", { class: "error-text" }, errorMessage(e)))),
      h("div", { class: "task-actions" }, button("Fermer", { variant: "accent", onclick: () => void win.close() })),
    );
  }

  function setTitle(text: string): void {
    title.textContent = text;
    void win.setTitle(text);
  }

  // Mot de passe demandé dans la carte elle-même : la fenêtre est trop petite pour un dialogue.
  async function extractWithPassword(
    archive: string,
    mode: ExtractMode,
    dest: string | null,
    run: typeof track,
  ): Promise<ExtractReport> {
    let password: string | null = null;
    for (let attempt = 0; ; attempt++) {
      try {
        return await run((onProgress) => extract({ archive, mode, dest, password, overwrite: settings().overwrite }, onProgress));
      } catch (e) {
        if (!(isCmdError(e) && e.kind === "password")) throw e;
        password = await promptInline(fmt.baseName(archive), attempt > 0);
        if (password === null) throw new Cancelled();
      }
    }
  }

  function promptInline(name: string, retry: boolean): Promise<string | null> {
    return new Promise((resolve) => {
      const saved = [...card.childNodes];
      const input = h("input", { type: "password", class: "field", placeholder: "Mot de passe" });
      const done = (value: string | null) => {
        card.replaceChildren(...saved);
        resolve(value);
      };
      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && input.value) done(input.value);
        if (e.key === "Escape") done(null);
      });
      card.replaceChildren(
        h("h2", {}, icon(Glyph.lock), retry ? ` Mot de passe incorrect` : ` « ${name} » est protégée`),
        input,
        h(
          "div",
          { class: "task-actions" },
          button("OK", { variant: "accent", onclick: () => input.value && done(input.value) }),
          button("Annuler", { variant: "standard", onclick: () => done(null) }),
        ),
      );
      input.focus();
    });
  }
}
