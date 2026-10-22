// Couleur d'accent calculée : à partir d'une teinte de départ (celle de WinSAR ou celle
// de Windows), on cherche en OKLCH la nuance la plus fidèle qui garantit le contraste
// WCAG : texte blanc lisible sur l'accent en thème clair, accent lisible sur le fond
// en thème sombre (et texte noir dessus). Même un rose vif ou un jaune deviennent sûrs.

type Rgb = [number, number, number];

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

function parseHex(hex: string): Rgb {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? [...h].map((c) => c + c).join("") : h.slice(0, 6);
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16) / 255) as Rgb;
}

function toHex(rgb: Rgb): string {
  return `#${rgb.map((c) => Math.round(Math.min(1, Math.max(0, c)) * 255).toString(16).padStart(2, "0")).join("")}`;
}

/** sRGB (0–1) → OKLCH (L 0–1, C, h en degrés). */
function toOklch([r, g, b]: Rgb): [number, number, number] {
  const [lr, lg, lb] = [r, g, b].map(toLinear);
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [L, Math.hypot(a, bb), (Math.atan2(bb, a) * 180) / Math.PI];
}

/** OKLCH → sRGB linéaire, éventuellement hors gamut. */
function fromOklchLinear(L: number, C: number, h: number): Rgb {
  const a = C * Math.cos((h * Math.PI) / 180);
  const b = C * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

/** Couleur affichable la plus proche : on réduit la saturation jusqu'à entrer dans sRGB. */
function fromOklch(L: number, C: number, h: number): Rgb {
  let c = C;
  for (let i = 0; i < 40; i++) {
    const lin = fromOklchLinear(L, c, h);
    if (lin.every((v) => v >= -1e-4 && v <= 1 + 1e-4)) return lin.map(toGamma) as Rgb;
    c *= 0.94;
  }
  return fromOklchLinear(L, 0, h).map(toGamma) as Rgb;
}

function luminance([r, g, b]: Rgb): number {
  return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

export function contrast(a: string, b: string): number {
  const [la, lb] = [luminance(parseHex(a)), luminance(parseHex(b))].sort((x, y) => y - x);
  return (la + 0.05) / (lb + 0.05);
}

/**
 * Dichotomie sur la luminosité : `ok` est faux en `from`, vrai en `to` (dans un sens
 * ou dans l'autre). Rend la valeur satisfaisante la plus proche de `from`.
 */
function boundary(from: number, to: number, ok: (L: number) => boolean): number {
  for (let i = 0; i < 30; i++) {
    const mid = (from + to) / 2;
    if (ok(mid)) to = mid;
    else from = mid;
  }
  return to;
}

/** Même teinte et même saturation que `seed`, à la luminosité OKLCH `L` (0 à 1). */
export function tone(seed: string, L: number): string {
  const [, C, h] = toOklch(parseHex(seed));
  return toHex(fromOklch(L, C, h));
}

export interface Accent {
  /** Thème clair : fond des boutons (texte blanc dessus), liens, sélection. */
  light: string;
  /** Thème sombre : même rôle, avec du texte noir dessus. */
  dark: string;
}

/** Références du contraste : texte sur l'accent clair, fond de fenêtre sombre (sans Mica). */
const LIGHT_TEXT_ON_ACCENT = "#ffffff";
const DARK_WINDOW = "#202020";

export function accentFrom(seed: string): Accent {
  const [L0, C, h] = toOklch(parseHex(seed));
  // Clair : la nuance la plus claire (donc la plus proche d'un accent vif) qui garde
  // 4,5:1 avec le texte blanc. Si la couleur de départ suffit déjà, on la garde.
  const lightOk = (L: number) => contrast(toHex(fromOklch(L, C, h)), LIGHT_TEXT_ON_ACCENT) >= 4.5;
  const lightL = lightOk(L0) ? L0 : boundary(L0, 0.1, lightOk);
  // Sombre : la nuance la plus sombre qui garde 4,5:1 avec le fond de fenêtre ; au
  // moins aussi claire que la teinte de départ, pour rester lumineuse.
  const darkOk = (L: number) => contrast(toHex(fromOklch(L, C, h)), DARK_WINDOW) >= 4.5;
  const darkL = Math.max(darkOk(L0) ? L0 : boundary(L0, 0.99, darkOk), 0.72);
  return { light: toHex(fromOklch(lightL, C, h)), dark: toHex(fromOklch(darkL, C, h)) };
}
