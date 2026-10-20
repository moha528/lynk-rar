// Barre de commandes adaptative, à la manière de l'Explorateur de Windows 11 :
// quand la place manque, les libellés disparaissent d'abord, puis les commandes
// les moins importantes passent dans un menu « … ».

import { button, Glyph, h, icon, menuBelow, type MenuItem } from "./ui";

export interface Tool {
  id: string;
  label: string;
  glyph: string;
  title?: string;
  disabled?: boolean;
  accent?: boolean;
  /** Ouvre un menu au lieu d'agir directement. */
  menu?: () => (MenuItem | "-")[];
  action?: () => void;
  /** Plus c'est élevé, plus la commande reste visible longtemps. */
  priority: number;
}

export type ToolbarItem = Tool | "sep";

const MORE = "";

export class Toolbar {
  readonly el = h("div", { class: "toolbar", role: "toolbar" });
  private items: ToolbarItem[] = [];

  constructor() {
    new ResizeObserver(() => this.layout()).observe(this.el);
  }

  set(items: ToolbarItem[]): void {
    this.items = items;
    this.layout();
  }

  private render(hidden: Set<string>, iconOnly: Set<string> = new Set()): void {
    const visible = this.items.filter((i) => i === "sep" || !hidden.has(i.id));
    // Pas de séparateur en tête, en fin, ni deux d'affilée.
    const cleaned = visible.filter((i, n) => i !== "sep" || (n > 0 && n < visible.length - 1 && visible[n - 1] !== "sep"));
    const nodes: HTMLElement[] = cleaned.map((i) => {
      if (i === "sep") return h("div", { class: "sep" });
      const b = this.button(i);
      b.classList.toggle("icon-only", iconOnly.has(i.id));
      return b;
    });
    const overflow = this.items.filter((i): i is Tool => i !== "sep" && hidden.has(i.id));
    if (overflow.length) {
      const more: HTMLButtonElement = button("", {
        glyph: MORE,
        title: "Plus d'options",
        onclick: () => menuBelow(more, overflow.flatMap((t) => this.asMenu(t))),
      });
      more.classList.add("more");
      nodes.push(more);
    }
    this.el.replaceChildren(...nodes);
  }

  private button(t: Tool): HTMLButtonElement {
    const b: HTMLButtonElement = button(t.label, {
      glyph: t.glyph,
      variant: t.accent && !t.disabled ? "accent" : "subtle",
      disabled: t.disabled,
      title: t.title ?? t.label,
      onclick: () => (t.menu ? menuBelow(b, t.menu()) : t.action?.()),
    });
    if (t.menu) b.append(icon(Glyph.down, "small chevron"));
    b.dataset.tool = t.id;
    b.setAttribute("aria-label", t.label);
    return b;
  }

  private asMenu(t: Tool): (MenuItem | "-")[] {
    if (t.menu) return ["-", ...t.menu(), "-"];
    return [{ label: t.label, glyph: t.glyph, disabled: t.disabled, action: () => t.action?.() }];
  }

  private overflows(): boolean {
    return this.el.scrollWidth > this.el.clientWidth + 1;
  }

  /** Libellés complets ; puis, de la moins à la plus importante, chaque commande perd
   *  son libellé ; puis passe dans « … ». Les commandes principales gardent leur texte
   *  le plus longtemps possible. */
  private layout(): void {
    const tools = this.items.filter((i): i is Tool => i !== "sep").sort((a, b) => a.priority - b.priority);
    const hidden = new Set<string>();
    const iconOnly = new Set<string>();
    this.render(hidden, iconOnly);
    if (!this.overflows()) return;
    for (const t of tools) {
      iconOnly.add(t.id);
      this.render(hidden, iconOnly);
      if (!this.overflows()) return;
    }
    for (const t of tools) {
      hidden.add(t.id);
      this.render(hidden, iconOnly);
      if (!this.overflows()) return;
    }
  }
}