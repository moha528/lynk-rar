import { size } from "./format";
import type { Job, ProgressEvent } from "./api";

type Child = Node | string | null | undefined | false;
type Props = Record<string, unknown> & { class?: string };

/** Création d'élément concise : `h("button", { class: "btn", onclick }, "OK")`. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null || value === false) continue;
    if (key === "class") el.className = String(value);
    else if (key.startsWith("on") && typeof value === "function") {
      el.addEventListener(key.slice(2), value as EventListener);
    } else if (key in el) (el as unknown as Record<string, unknown>)[key] = value;
    else el.setAttribute(key, value === true ? "" : String(value));
  }
  for (const c of children) if (c !== null && c !== undefined && c !== false) el.append(c);
  return el;
}

/** Glyphes de la police système Segoe Fluent Icons (repli : Segoe MDL2 Assets). */
export const Glyph = {
  open: "",
  add: "",
  extract: "",
  folder: "",
  file: "",
  up: "",
  back: "",
  chevron: "",
  down: "",
  search: "",
  lock: "",
  reveal: "",
  cancel: "",
  check: "",
  error: "",
  archive: "",
  delete: "",
} as const;

export function icon(glyph: string, cls = ""): HTMLSpanElement {
  return h("span", { class: `icon ${cls}`, "aria-hidden": "true" }, glyph);
}

export function button(
  label: string,
  opts: { glyph?: string; variant?: "accent" | "standard" | "subtle"; onclick?: (e: MouseEvent) => void; title?: string; disabled?: boolean } = {},
): HTMLButtonElement {
  return h(
    "button",
    {
      class: `btn ${opts.variant ?? "subtle"}`,
      onclick: opts.onclick,
      title: opts.title,
      disabled: opts.disabled,
      type: "button",
    },
    opts.glyph ? icon(opts.glyph) : null,
    label ? h("span", {}, label) : null,
  );
}

export interface MenuItem {
  label: string;
  glyph?: string;
  hint?: string;
  disabled?: boolean;
  action: () => void;
}

/** Menu contextuel ou déroulant, refermé au clic extérieur et sur Échap (API popover). */
export function showMenu(items: (MenuItem | "-")[], at: { x: number; y: number }): void {
  document.querySelector(".menu")?.remove();
  const menu = h("div", { class: "menu", popover: "auto" });
  // Pas de séparateur en tête, en fin, ni deux à la suite.
  items = items.filter((it, i) => it !== "-" || (i > 0 && i < items.length - 1 && items[i - 1] !== "-"));
  for (const item of items) {
    if (item === "-") {
      menu.append(h("div", { class: "menu-sep" }));
      continue;
    }
    menu.append(
      h(
        "button",
        {
          class: "menu-item",
          disabled: item.disabled,
          type: "button",
          onclick: () => {
            menu.hidePopover();
            item.action();
          },
        },
        icon(item.glyph ?? ""),
        h("span", { class: "menu-label" }, item.label),
        item.hint ? h("span", { class: "menu-hint" }, item.hint) : null,
      ),
    );
  }
  menu.addEventListener("toggle", (e) => {
    if ((e as ToggleEvent).newState === "closed") menu.remove();
  });
  document.body.append(menu);
  menu.showPopover();
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(at.x, innerWidth - r.width - 8)}px`;
  menu.style.top = `${Math.min(at.y, innerHeight - r.height - 8)}px`;
}

export function menuBelow(anchor: HTMLElement, items: (MenuItem | "-")[]): void {
  const r = anchor.getBoundingClientRect();
  showMenu(items, { x: r.left, y: r.bottom + 4 });
}

/** Boîte de dialogue modale native (`<dialog>`), résolue à la fermeture. */
export function modal<T>(
  build: (close: (value: T) => void) => { title: string; body: Node; actions: HTMLElement[]; className?: string },
  dismissed: T,
): Promise<T> {
  return new Promise((resolve) => {
    let settled = false;
    const dialog = h("dialog", { class: "dialog" });
    const close = (value: T) => {
      if (settled) return;
      settled = true;
      dialog.close();
      dialog.remove();
      resolve(value);
    };
    const { title, body, actions, className } = build(close);
    if (className) dialog.classList.add(className);
    dialog.append(
      h("div", { class: "dialog-content" }, h("h2", {}, title), body),
      h("div", { class: "dialog-actions" }, ...actions),
    );
    dialog.addEventListener("cancel", () => close(dismissed));
    document.body.append(dialog);
    dialog.showModal();
  });
}

export function askPassword(archiveName: string, retry: boolean): Promise<string | null> {
  return modal<string | null>((close) => {
    const input = h("input", { type: "password", class: "field", placeholder: "Mot de passe", autofocus: true });
    const ok = button("OK", { variant: "accent", onclick: () => close(input.value) });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && input.value) close(input.value);
    });
    return {
      title: "Mot de passe requis",
      body: h(
        "div",
        { class: "stack" },
        h("p", {}, retry ? `Mot de passe incorrect pour « ${archiveName} ».` : `« ${archiveName} » est protégée par un mot de passe.`),
        input,
      ),
      actions: [ok, button("Annuler", { variant: "standard", onclick: () => close(null) })],
    };
  }, null);
}

export function alertError(title: string, message: string): Promise<void> {
  return modal<void>((close) => ({
    title,
    body: h("p", { class: "error-text" }, message),
    actions: [button("Fermer", { variant: "accent", onclick: () => close() })],
  }), undefined);
}

export function toast(message: string, action?: { label: string; run: () => void }): void {
  const host = document.querySelector(".toasts") ?? document.body.appendChild(h("div", { class: "toasts" }));
  const el = h(
    "div",
    { class: "toast" },
    icon(Glyph.check, "ok"),
    h("span", { class: "toast-text" }, message),
    action ? button(action.label, { variant: "standard", onclick: () => { action.run(); el.remove(); } }) : null,
  );
  host.append(el);
  setTimeout(() => el.remove(), 7000);
}

/** Barre de progression avec débit et temps restant, alimentée par les événements Rust. */
export class ProgressView {
  readonly root: HTMLElement;
  private bar = h("div", { class: "bar-fill" });
  private current = h("div", { class: "progress-current" });
  private stats = h("div", { class: "progress-stats" });
  private started = performance.now();

  constructor(title: HTMLElement) {
    this.root = h("div", { class: "progress" }, title, this.current, h("div", { class: "bar" }, this.bar), this.stats);
  }

  reset(): void {
    this.started = performance.now();
    this.bar.style.width = "0%";
    this.current.textContent = "";
    this.stats.textContent = "Préparation…";
  }

  update(p: ProgressEvent): void {
    const pct = p.total > 0 ? Math.min(100, (p.done / p.total) * 100) : 0;
    this.bar.style.width = `${pct}%`;
    this.current.textContent = p.current;
    const elapsed = (performance.now() - this.started) / 1000;
    const speed = elapsed > 0.3 ? p.done / elapsed : 0;
    const remaining = speed > 0 && p.total > p.done ? (p.total - p.done) / speed : 0;
    const parts = [`${size(p.done)} sur ${size(p.total)}`];
    if (speed > 0) parts.push(`${size(speed)}/s`);
    if (remaining >= 1) parts.push(`${formatDuration(remaining)} restantes`);
    this.stats.textContent = parts.join(" · ");
  }
}

function formatDuration(s: number): string {
  if (s < 60) return `${Math.ceil(s)} s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min ${Math.round(s % 60)} s` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** Progression dans une feuille modale (mode explorateur). */
export async function withProgress<T>(title: string, start: (onProgress: (p: ProgressEvent) => void) => Job<T>): Promise<T> {
  const view = new ProgressView(h("h2", {}, title));
  view.reset();
  const dialog = h("dialog", { class: "dialog progress-dialog" });
  const job = start((p) => view.update(p));
  dialog.append(
    h("div", { class: "dialog-content" }, view.root),
    h("div", { class: "dialog-actions" }, button("Annuler", { variant: "standard", onclick: () => job.cancel() })),
  );
  dialog.addEventListener("cancel", (e) => {
    e.preventDefault();
    job.cancel();
  });
  document.body.append(dialog);
  dialog.showModal();
  try {
    return await job.done;
  } finally {
    dialog.close();
    dialog.remove();
  }
}
