// Page des réglages, à la manière de l'application Paramètres de Windows 11.

import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";

import { api, errorMessage, type FormatInfo, type Overwrite } from "./api";
import logoUrl from "../app-icon.svg";
import { applyAppearance, MENU_ENTRIES, settings, updateSettings, type AccentSource, type StartPlace, type Theme } from "./settings";
import { alertError, button, Glyph, h } from "./ui";
import { checkForUpdate, REPO_URL } from "./updates";

/** Extensions proposées à l'association, dans l'ordre d'affichage. */
const FILE_TYPES = [".zip", ".7z", ".rar", ".tar", ".gz", ".tgz", ".bz2", ".xz", ".zst", ".001"];

export const LEVELS = ["Stockage", "La plus rapide", "Rapide", "Rapide", "Normale", "Normale", "Normale", "Bonne", "Maximale", "Ultra"];

function card(title: string, desc: string | null, control: Node, extra?: Node): HTMLElement {
  return h(
    "div",
    { class: "setting" },
    h("div", { class: "setting-text" }, h("div", { class: "setting-title" }, title), desc ? h("div", { class: "setting-desc" }, desc) : null),
    h("div", { class: "setting-control" }, control),
    extra ?? null,
  );
}

function toggle(checked: boolean, onChange: (v: boolean) => void, label: string): HTMLLabelElement {
  const input = h("input", { type: "checkbox", checked, "aria-label": label });
  input.addEventListener("change", () => onChange(input.checked));
  return h("label", { class: "switch" }, input, h("span", { class: "track" }), h("span", { class: "switch-state" }, checked ? "Activé" : "Désactivé"));
}

function select<V extends string>(value: V, options: [V, string][], onChange: (v: V) => void): HTMLSelectElement {
  const sel = h("select", { class: "field" }, ...options.map(([v, l]) => h("option", { value: v, selected: v === value }, l)));
  sel.addEventListener("change", () => onChange(sel.value as V));
  return sel;
}

function section(title: string, ...cards: (Node | null)[]): HTMLElement {
  return h("section", { class: "settings-section" }, h("h2", {}, title), ...cards);
}

export async function renderSettings(formats: FormatInfo[]): Promise<HTMLElement> {
  const s = settings();
  const root = h("div", { class: "settings" }, h("h1", {}, "Paramètres"));

  // ——— Général ———
  const customPath = h("span", { class: "muted small path" }, s.startPath || "Aucun dossier choisi");
  const pickStart = button("Choisir…", {
    variant: "standard",
    onclick: async () => {
      const picked = await open({ directory: true, title: "Dossier de démarrage" });
      if (typeof picked === "string") {
        updateSettings({ startPath: picked, start: "custom" });
        customPath.textContent = picked;
        startSel.value = "custom";
        startSub.hidden = false;
      }
    },
  });
  const startSel = select<StartPlace>(
    s.start,
    [
      ["home", "Accueil"],
      ["downloads", "Téléchargements"],
      ["last", "Dernier emplacement"],
      ["custom", "Un dossier précis"],
    ],
    (v) => {
      updateSettings({ start: v });
      startSub.hidden = v !== "custom";
    },
  );
  // Le choix du dossier n'apparaît que pour « Un dossier précis ».
  const startSub = h("div", { class: "setting-sub", hidden: s.start !== "custom" }, customPath, pickStart);
  root.append(
    section(
      "Général",
      card("Au démarrage, ouvrir", "WinSAR est aussi un explorateur de fichiers : choisissez où il s'ouvre.", startSel, startSub),
      card("Fichiers cachés", "Afficher les fichiers et dossiers cachés ou système.", toggle(s.showHidden, (v) => updateSettings({ showHidden: v }), "Fichiers cachés")),
    ),
  );

  // ——— Apparence ———
  const themes = h(
    "div",
    { class: "segmented", role: "radiogroup" },
    ...([
      ["system", "Système"],
      ["light", "Clair"],
      ["dark", "Sombre"],
    ] as [Theme, string][]).map(([v, l]) =>
      h(
        "button",
        {
          type: "button",
          class: `seg${s.theme === v ? " on" : ""}`,
          role: "radio",
          "aria-checked": s.theme === v ? "true" : "false",
          onclick: (e: MouseEvent) => {
            updateSettings({ theme: v });
            void applyAppearance();
            for (const b of themes.children) b.classList.toggle("on", b === e.currentTarget);
          },
        },
        l,
      ),
    ),
  );
  const accents = h(
    "div",
    { class: "segmented", role: "radiogroup" },
    ...([
      ["winsar", "WinSAR"],
      ["windows", "Windows"],
    ] as [AccentSource, string][]).map(([v, l]) =>
      h(
        "button",
        {
          type: "button",
          class: `seg${s.accent === v ? " on" : ""}`,
          role: "radio",
          "aria-checked": s.accent === v ? "true" : "false",
          onclick: (e: MouseEvent) => {
            updateSettings({ accent: v });
            void applyAppearance();
            for (const b of accents.children) {
              b.classList.toggle("on", b === e.currentTarget);
              b.setAttribute("aria-checked", b === e.currentTarget ? "true" : "false");
            }
          },
        },
        h("span", { class: `swatch swatch-${v}`, "aria-hidden": "true" }),
        l,
      ),
    ),
  );
  root.append(
    section(
      "Apparence",
      card("Thème", "Suivre Windows ou forcer le mode clair ou sombre.", themes),
      card("Couleur d'accent", "Celle de WinSAR, ou celle choisie dans Windows. Elle est ajustée pour rester lisible dans les deux thèmes.", accents),
      card("Effet Mica", "Laisser transparaître le fond d'écran derrière la fenêtre (Windows 11).", toggle(s.mica, (v) => {
        updateSettings({ mica: v });
        void applyAppearance();
      }, "Effet Mica")),
    ),
  );

  // ——— Extraction ———
  root.append(
    section(
      "Extraction",
      card(
        "Si un fichier existe déjà",
        null,
        select<Overwrite>(
          s.overwrite,
          [
            ["rename", "Garder les deux (renommer)"],
            ["replace", "Remplacer"],
            ["skip", "Ne pas extraire"],
          ],
          (v) => updateSettings({ overwrite: v }),
        ),
      ),
      card("Ouvrir le dossier après l'extraction", null, toggle(s.openFolderAfterExtract, (v) => updateSettings({ openFolderAfterExtract: v }), "Ouvrir le dossier")),
      card(
        "Notifications",
        "Prévenir à la fin d'une tâche lancée depuis le menu contextuel de l'Explorateur.",
        toggle(s.notifications, (v) => updateSettings({ notifications: v }), "Notifications"),
      ),
    ),
  );

  // ——— Compression ———
  const levelLabel = h("span", { class: "muted level-label" }, LEVELS[s.defaultLevel]);
  const level = h("input", { type: "range", min: 0, max: 9, value: s.defaultLevel, class: "slider", "aria-label": "Niveau" });
  level.addEventListener("input", () => {
    levelLabel.textContent = LEVELS[Number(level.value)];
    updateSettings({ defaultLevel: Number(level.value) });
  });
  root.append(
    section(
      "Compression",
      card(
        "Format par défaut",
        "Utilisé par « Nouvelle archive » et le menu contextuel.",
        select(s.defaultFormat, formats.filter((f) => f.canCreate).map((f) => [f.id, f.label]), (v) => updateSettings({ defaultFormat: v })),
      ),
      card("Niveau par défaut", null, h("div", { class: "row" }, level, levelLabel)),
    ),
  );

  // ——— Intégration Windows ———
  const types = h("div", { class: "filetypes" });
  const refreshTypes = async () => {
    const status = await api.fileTypesStatus(FILE_TYPES).catch(() => ({}) as Awaited<ReturnType<typeof api.fileTypesStatus>>);
    types.replaceChildren(
      ...FILE_TYPES.map((ext) => {
        const st = status[ext];
        const box = h("input", { type: "checkbox", checked: !!st?.registered, "aria-label": `Associer ${ext}` });
        box.addEventListener("change", async () => {
          try {
            await api.registerFileTypes([ext], box.checked);
          } catch (e) {
            await alertError("Association impossible", errorMessage(e));
          }
          await refreshTypes();
        });
        const chip = st?.default
          ? h("span", { class: "chip ok" }, "Par défaut")
          : st?.registered
            ? h("span", { class: "chip" }, "Proposé")
            : h("span", { class: "chip off" }, st?.current ? `Ouvert par ${st.current}` : "Non associé");
        return h("label", { class: "filetype" }, box, h("span", { class: "ext" }, ext), chip);
      }),
    );
  };
  void refreshTypes();
  const all = button("Tout associer", {
    variant: "standard",
    onclick: async () => {
      await api.registerFileTypes(FILE_TYPES, true).catch((e) => alertError("Association impossible", errorMessage(e)));
      await refreshTypes();
    },
  });
  const defaults = button("Choisir WinSAR par défaut…", { variant: "accent", glyph: Glyph.reveal, onclick: () => void api.openDefaultApps() });

  const menu = h(
    "div",
    { class: "menu-entries" },
    ...MENU_ENTRIES.map(({ bit, label }) =>
      h(
        "label",
        { class: "check" },
        (() => {
          const box = h("input", { type: "checkbox", checked: (s.menuHidden & (1 << bit)) === 0 });
          box.addEventListener("change", () => {
            const mask = box.checked ? settings().menuHidden & ~(1 << bit) : settings().menuHidden | (1 << bit);
            updateSettings({ menuHidden: mask });
            void api.setContextMenuHidden(mask);
          });
          return box;
        })(),
        h("span", {}, label),
      ),
    ),
  );

  root.append(
    section(
      "Intégration à Windows",
      h(
        "div",
        { class: "setting column" },
        h("div", { class: "setting-text" }, h("div", { class: "setting-title" }, "Associations de fichiers"), h(
          "div",
          { class: "setting-desc" },
          "Cochez les types que WinSAR doit savoir ouvrir. Windows réserve à l'utilisateur le choix de l'application par défaut : confirmez-le ensuite dans Paramètres.",
        )),
        types,
        h("div", { class: "row" }, defaults, all),
      ),
      h(
        "div",
        { class: "setting column" },
        h("div", { class: "setting-text" }, h("div", { class: "setting-title" }, "Menu contextuel de l'Explorateur"), h(
          "div",
          { class: "setting-desc" },
          "Entrées affichées dans le sous-menu « WinSAR ». Effet immédiat, sans redémarrer l'Explorateur.",
        )),
        menu,
      ),
      card(
        "Menu contextuel classique",
        "Entrées WinSAR dans « Afficher plus d'options ». L'installeur l'active quand le menu principal de Windows 11 n'a pas pu être installé (version non signée).",
        toggle(await api.classicMenuStatus().catch(() => false), (v) => {
          api.setClassicMenu(v).catch((e) => alertError("Menu classique", errorMessage(e)));
        }, "Menu contextuel classique"),
      ),
    ),
  );

  // ——— À propos ———
  const version = await getVersion().catch(() => "");
  const lastCheck = s.lastUpdateCheck ? `Dernière vérification : ${new Date(s.lastUpdateCheck).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" })}.` : "Jamais vérifié.";
  const updateText = h("div", { class: "setting-desc", role: "status" }, lastCheck);
  const updateActions = h("div", { class: "setting-sub", hidden: true });
  const checkNow: HTMLButtonElement = button("Rechercher maintenant", {
    variant: "standard",
    glyph: Glyph.update,
    onclick: async () => {
      checkNow.disabled = true;
      updateText.textContent = "Recherche en cours…";
      updateActions.replaceChildren();
      updateActions.hidden = true;
      try {
        const status = await checkForUpdate();
        if (status.available && status.latest) {
          const latest = status.latest;
          updateText.textContent = `WinSAR ${latest.version} est disponible (vous avez la ${status.current}).`;
          updateActions.hidden = false;
          updateActions.replaceChildren(button("Voir la nouvelle version", { variant: "accent", glyph: Glyph.reveal, onclick: () => void api.openUrl(latest.url) }));
        } else {
          updateText.textContent = status.latest ? `WinSAR ${status.current} est à jour.` : "Aucune version publiée pour l'instant.";
        }
      } catch (e) {
        updateText.textContent = `Vérification impossible : ${errorMessage(e)}`;
      } finally {
        checkNow.disabled = false;
      }
    },
  });
  root.append(
    section(
      "Mises à jour",
      card(
        "Rechercher les mises à jour au démarrage",
        "Une fois par jour au plus, WinSAR lit la dernière version publiée sur GitHub. Rien n'est envoyé ni installé sans vous.",
        toggle(s.checkUpdates, (v) => updateSettings({ checkUpdates: v }), "Mises à jour automatiques"),
      ),
      h(
        "div",
        { class: "setting" },
        h("div", { class: "setting-text" }, h("div", { class: "setting-title" }, `Version installée : ${version}`), updateText),
        h("div", { class: "setting-control" }, checkNow),
        updateActions,
      ),
    ),
  );
  root.append(
    section(
      "À propos",
      h(
        "div",
        { class: "setting about" },
        h("img", { src: logoUrl, class: "about-logo", alt: "" }),
        h(
          "div",
          { class: "setting-text" },
          h("div", { class: "setting-title" }, `WinSAR ${version}`),
          h("div", { class: "setting-desc" }, "Archives rapides, natives et libres pour Windows. Logiciel libre sous licence MIT."),
          h(
            "div",
            { class: "setting-desc small" },
            "Extraction RAR grâce au code source UnRAR © Alexander Roshal, utilisé selon sa licence (lecture seule : WinSAR ne crée pas d'archives RAR).",
          ),
        ),
        h(
          "div",
          { class: "setting-control column-end" },
          button("Code source", { variant: "standard", glyph: "", onclick: () => void api.openUrl(REPO_URL) }),
          button("Signaler un problème", { variant: "standard", glyph: Glyph.error, onclick: () => void api.openUrl(`${REPO_URL}/issues`) }),
        ),
      ),
    ),
  );
  return root;
}
