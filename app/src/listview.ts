// Liste en mode « détails », comme celle de l'Explorateur : virtualisée (seules les
// lignes visibles existent dans le DOM), colonnes redimensionnables et triables,
// sélection souris/clavier, recherche par frappe.

import { h, icon, Glyph } from "./ui";

export interface Column<T> {
  id: string;
  label: string;
  /** Largeur par défaut, en px. */
  width: number;
  min?: number;
  align?: "end";
  /** Colonne masquée quand la liste est plus étroite que cette largeur (affichage étroit). */
  hideBelow?: number;
  cell(item: T): Node | string;
  /** Texte de la cellule, pour l'ajustement automatique de la largeur. */
  text(item: T): string;
  sortValue(item: T): string | number;
}

export interface ListOptions<T> {
  key(item: T): string;
  name(item: T): string;
  isDir(item: T): boolean;
  columns: Column<T>[];
  widths: Record<string, number>;
  saveWidths(widths: Record<string, number>): void;
  onActivate(item: T): void;
  /** `items` vide : clic droit dans le vide de la liste. */
  onContextMenu(items: T[], e: MouseEvent): void;
  onSelection(): void;
  onDragStart(): void;
  /** Raccourcis propres à la vue (Retour arrière, Suppr…) ; `true` si traité. */
  onKey(e: KeyboardEvent): boolean;
}

const ROW = 32;
const OVERSCAN = 10;
const collator = new Intl.Collator("fr", { numeric: true, sensitivity: "base" });

export class ListView<T> {
  readonly el = h("div", { class: "list", tabIndex: 0, role: "grid" });
  private head = h("div", { class: "list-head grid-row", role: "row" });
  private body = h("div", { class: "list-body" });
  private window = h("div", { class: "list-window" });
  private emptyEl = h("div", { class: "list-empty muted" });
  private items: T[] = [];
  private index = new Map<string, number>();
  readonly selected = new Set<string>();
  /** Élément « courant » (curseur clavier, ancre des sélections étendues). */
  private cursor: string | null = null;
  private sortKey: string;
  private asc = true;
  private visible: Column<T>[] = [];
  private typed = "";
  private typedAt = 0;
  private dragFrom: { x: number; y: number } | null = null;

  constructor(private readonly o: ListOptions<T>, sort: { key: string; asc: boolean }) {
    this.sortKey = sort.key;
    this.asc = sort.asc;
    this.body.append(this.window);
    this.el.append(this.head, this.body, this.emptyEl);
    this.el.addEventListener("scroll", () => this.renderRows(), { passive: true });
    this.el.addEventListener("keydown", (e) => this.onKey(e));
    this.body.addEventListener("mousedown", (e) => this.onMouseDown(e));
    this.body.addEventListener("dblclick", (e) => {
      const item = this.itemAt(e);
      if (item) this.o.onActivate(item);
    });
    this.el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const item = this.itemAt(e);
      if (item && !this.selected.has(this.o.key(item))) this.selectOnly(this.o.key(item));
      if (!item && (e.target as HTMLElement).closest(".list-head")) return;
      this.o.onContextMenu(item ? this.selectedItems() : [], e);
    });
    this.body.addEventListener("mousemove", (e) => {
      if (!this.dragFrom || !(e.buttons & 1)) return;
      if (Math.hypot(e.clientX - this.dragFrom.x, e.clientY - this.dragFrom.y) < 6) return;
      this.dragFrom = null;
      this.o.onDragStart();
    });
    document.addEventListener("mouseup", () => (this.dragFrom = null));
    new ResizeObserver(() => this.layout()).observe(this.el);
  }

  // ——— Données ———

  setItems(items: T[], opts: { select?: string[]; empty?: string } = {}): void {
    this.items = items;
    this.sortItems();
    this.selected.clear();
    for (const k of opts.select ?? []) if (this.index.has(k)) this.selected.add(k);
    this.cursor = this.anchor = opts.select?.find((k) => this.index.has(k)) ?? null;
    this.emptyEl.textContent = opts.empty ?? "";
    this.el.classList.toggle("is-empty", items.length === 0);
    this.el.scrollTop = 0;
    this.layout(true);
    if (this.cursor) this.ensureVisible(this.index.get(this.cursor)!);
    this.o.onSelection();
  }

  /** Redessine les lignes visibles (icônes arrivées, réglage modifié…). */
  refresh(): void {
    this.renderRows();
  }

  all(): T[] {
    return this.items;
  }

  selectedItems(): T[] {
    return this.items.filter((i) => this.selected.has(this.o.key(i)));
  }

  focus(): void {
    this.el.focus({ preventScroll: true });
  }

  sort(): { key: string; asc: boolean } {
    return { key: this.sortKey, asc: this.asc };
  }

  private sortItems(): void {
    const col = this.o.columns.find((c) => c.id === this.sortKey) ?? this.o.columns[0];
    const dir = this.asc ? 1 : -1;
    this.items.sort((a, b) => {
      const da = this.o.isDir(a);
      if (da !== this.o.isDir(b)) return da ? -1 : 1; // dossiers d'abord, comme l'Explorateur
      const va = col.sortValue(a);
      const vb = col.sortValue(b);
      const c = typeof va === "string" ? collator.compare(va, String(vb)) : va - Number(vb);
      return (c || collator.compare(this.o.name(a), this.o.name(b))) * dir;
    });
    this.index = new Map(this.items.map((i, n) => [this.o.key(i), n]));
  }

  // ——— Colonnes ———

  private width(c: Column<T>): number {
    return Math.max(c.min ?? 48, this.o.widths[c.id] ?? c.width);
  }

  private layout(force = false): void {
    const w = this.el.clientWidth;
    const visible = this.o.columns.filter((c) => !c.hideBelow || w >= c.hideBelow);
    if (force || visible.length !== this.visible.length || visible.some((c, i) => c !== this.visible[i])) {
      this.visible = visible;
      this.renderHead();
    }
    this.applyWidths();
    this.renderRows();
  }

  private applyWidths(): void {
    const widths = this.visible.map((c) => this.width(c));
    this.el.style.setProperty("--cols", widths.map((x) => `${x}px`).join(" "));
    this.el.style.setProperty("--row-width", `${widths.reduce((a, b) => a + b, 0) + 16}px`);
  }

  private renderHead(): void {
    this.head.replaceChildren(
      ...this.visible.map((c) => {
        const sorted = this.sortKey === c.id;
        const cell = h(
          "div",
          { class: `head-cell${c.align === "end" ? " end" : ""}${sorted ? " sorted" : ""}`, role: "columnheader" },
          h(
            "button",
            { class: "head-label", type: "button", onclick: () => this.toggleSort(c.id) },
            h("span", { class: "ellipsis" }, c.label),
            sorted ? icon(this.asc ? "" : Glyph.down, "small") : null,
          ),
          h("span", {
            class: "resizer",
            title: "Glisser pour redimensionner · double-clic pour ajuster",
            onpointerdown: (e: PointerEvent) => this.startResize(e, c),
            ondblclick: () => this.autoFit(c),
          }),
        );
        return cell;
      }),
    );
  }

  private toggleSort(id: string): void {
    this.asc = this.sortKey === id ? !this.asc : true;
    this.sortKey = id;
    this.sortItems();
    this.renderHead();
    this.renderRows();
  }

  private startResize(e: PointerEvent, c: Column<T>): void {
    e.preventDefault();
    e.stopPropagation();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startW = this.width(c);
    document.body.classList.add("resizing");
    const move = (ev: PointerEvent) => {
      this.o.widths[c.id] = Math.max(c.min ?? 48, Math.round(startW + ev.clientX - startX));
      this.applyWidths();
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      document.body.classList.remove("resizing");
      this.o.saveWidths(this.o.widths);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  }

  /** Largeur ajustée au contenu le plus long (mesuré sur 3 000 éléments au plus). */
  private autoFit(c: Column<T>): void {
    const ctx = document.createElement("canvas").getContext("2d");
    if (!ctx) return;
    const style = getComputedStyle(this.el);
    ctx.font = `${style.fontSize} ${style.fontFamily}`;
    let max = ctx.measureText(c.label).width + 28;
    for (const item of this.items.slice(0, 3000)) max = Math.max(max, ctx.measureText(c.text(item)).width);
    const extra = c.id === "name" ? 58 : 28; // icône, marges, cadenas
    this.o.widths[c.id] = Math.min(900, Math.ceil(max + extra));
    this.applyWidths();
    this.o.saveWidths(this.o.widths);
  }

  // ——— Rendu virtualisé ———

  private renderRows(): void {
    this.body.style.height = `${this.items.length * ROW}px`;
    const offset = this.el.scrollTop - this.body.offsetTop;
    const first = Math.max(0, Math.floor(offset / ROW) - OVERSCAN);
    const last = Math.min(this.items.length, Math.ceil((offset + this.el.clientHeight) / ROW) + OVERSCAN);
    this.window.style.transform = `translateY(${first * ROW}px)`;
    const rows: HTMLElement[] = [];
    for (let i = first; i < last; i++) rows.push(this.row(this.items[i]));
    this.window.replaceChildren(...rows);
  }

  private row(item: T): HTMLElement {
    const key = this.o.key(item);
    const cls = `grid-row item${this.selected.has(key) ? " selected" : ""}${this.cursor === key ? " cursor" : ""}`;
    return h(
      "div",
      { class: cls, "data-key": key, role: "row", "aria-selected": this.selected.has(key) ? "true" : "false" },
      ...this.visible.map((c) => h("div", { class: `cell${c.align === "end" ? " end" : ""}` }, c.cell(item))),
    );
  }

  private updateSelectionClasses(): void {
    for (const el of this.window.children as HTMLCollectionOf<HTMLElement>) {
      const key = el.dataset.key ?? "";
      el.classList.toggle("selected", this.selected.has(key));
      el.classList.toggle("cursor", this.cursor === key);
      el.setAttribute("aria-selected", this.selected.has(key) ? "true" : "false");
    }
    this.o.onSelection();
  }

  private ensureVisible(i: number): void {
    const top = this.body.offsetTop + i * ROW;
    const view = this.el.clientHeight;
    const headH = this.head.offsetHeight;
    if (top - headH < this.el.scrollTop) this.el.scrollTop = top - headH;
    else if (top + ROW > this.el.scrollTop + view) this.el.scrollTop = top + ROW - view;
  }

  // ——— Sélection ———
  // `anchor` : origine d'une sélection étendue (Maj). `cursor` : élément courant.

  private anchor: string | null = null;

  private itemAt(e: Event): T | null {
    const row = (e.target as HTMLElement).closest<HTMLElement>(".item");
    const i = row ? this.index.get(row.dataset.key ?? "") : undefined;
    return i === undefined ? null : this.items[i];
  }

  private selectOnly(key: string): void {
    this.selected.clear();
    this.selected.add(key);
    this.cursor = this.anchor = key;
    this.updateSelectionClasses();
  }

  /** Sélectionne de l'ancre jusqu'à `to` ; `keep` conserve la sélection existante (Ctrl+Maj). */
  private extendTo(to: number, keep: boolean): void {
    const from = this.anchor !== null ? (this.index.get(this.anchor) ?? to) : to;
    if (!keep) this.selected.clear();
    for (let i = Math.min(from, to); i <= Math.max(from, to); i++) this.selected.add(this.o.key(this.items[i]));
    this.cursor = this.o.key(this.items[to]);
    if (this.anchor === null) this.anchor = this.cursor;
  }

  private onMouseDown(e: MouseEvent): void {
    this.focus();
    const item = this.itemAt(e);
    if (!item) {
      if (e.button === 0 && !e.ctrlKey && !e.shiftKey) {
        this.selected.clear();
        this.updateSelectionClasses();
      }
      return;
    }
    const key = this.o.key(item);
    if (e.button === 2 && this.selected.has(key)) return;
    if (e.button === 0) this.dragFrom = { x: e.clientX, y: e.clientY };
    if (e.shiftKey) {
      this.extendTo(this.index.get(key)!, e.ctrlKey);
    } else if (e.ctrlKey) {
      if (this.selected.has(key)) this.selected.delete(key);
      else this.selected.add(key);
      this.cursor = this.anchor = key;
    } else if (this.selected.has(key) && this.selected.size > 1) {
      // Clic dans une sélection multiple : on la garde pour un éventuel glisser,
      // et on ne la réduit à cet élément qu'au relâchement sans glisser.
      this.cursor = this.anchor = key;
      document.addEventListener("mouseup", () => this.dragFrom && this.selectOnly(key), { once: true });
    } else {
      this.selected.clear();
      this.selected.add(key);
      this.cursor = this.anchor = key;
    }
    this.updateSelectionClasses();
  }

  private moveCursor(to: number, shift: boolean, ctrl: boolean): void {
    if (!this.items.length) return;
    to = Math.max(0, Math.min(this.items.length - 1, to));
    const key = this.o.key(this.items[to]);
    if (shift) {
      this.extendTo(to, ctrl);
    } else {
      this.cursor = key;
      if (!ctrl) {
        this.selected.clear();
        this.selected.add(key);
        this.anchor = key;
      }
    }
    this.ensureVisible(to);
    this.renderRows();
    this.updateSelectionClasses();
  }

  private onKey(e: KeyboardEvent): void {
    if (this.o.onKey(e)) return;
    const page = Math.max(1, Math.floor(this.el.clientHeight / ROW) - 1);
    const at = this.cursor !== null ? (this.index.get(this.cursor) ?? -1) : -1;
    const moves: Record<string, number> = {
      ArrowDown: at + 1,
      ArrowUp: at < 0 ? 0 : at - 1,
      PageDown: at + page,
      PageUp: at - page,
      Home: 0,
      End: this.items.length - 1,
    };
    if (e.key in moves) {
      e.preventDefault();
      this.moveCursor(moves[e.key], e.shiftKey, e.ctrlKey);
    } else if (e.key === " " && e.ctrlKey && this.cursor !== null) {
      // Ctrl+Espace : bascule l'élément courant, comme dans l'Explorateur.
      e.preventDefault();
      if (this.selected.has(this.cursor)) this.selected.delete(this.cursor);
      else this.selected.add(this.cursor);
      this.updateSelectionClasses();
    } else if (e.key === "Enter") {
      const i = this.cursor !== null ? this.index.get(this.cursor) : undefined;
      if (i !== undefined) this.o.onActivate(this.items[i]);
    } else if (e.ctrlKey && e.key.toLowerCase() === "a") {
      e.preventDefault();
      this.items.forEach((i) => this.selected.add(this.o.key(i)));
      this.updateSelectionClasses();
    } else if (e.key.length === 1 && !e.ctrlKey && !e.altKey && e.key !== " ") {
      // Recherche par frappe : « rap » va au premier élément qui commence ainsi.
      const now = performance.now();
      this.typed = now - this.typedAt > 900 ? e.key : this.typed + e.key;
      this.typedAt = now;
      const needle = this.typed.toLocaleLowerCase("fr");
      const found = this.items.findIndex((i) => this.o.name(i).toLocaleLowerCase("fr").startsWith(needle));
      if (found >= 0) this.moveCursor(found, false, false);
    }
  }
}