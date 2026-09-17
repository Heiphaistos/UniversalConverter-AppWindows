// Modèle du filigrane. Toutes les grandeurs sont RELATIVES à la page
// (position 0–1, tailles en % du plus petit côté) : le même réglage donne le
// même rendu sur l'aperçu réduit, sur l'image pleine résolution et sur des
// pages de formats différents.

export type FillMode = "solid" | "linear" | "radial" | "none";
export type TextAlign = "left" | "center" | "right";

export interface ColorStop {
  offset: number; // 0–1
  color: string;  // #rrggbb
  alpha: number;  // 0–1
}

export interface WatermarkConfig {
  kind: "text" | "image";

  // Texte
  text: string;
  fontFamily: string;
  fontFile: string | null; // police importée (.ttf/.otf/.woff) — rechargée à la volée
  bold: boolean;
  italic: boolean;
  letterSpacing: number;   // em
  lineHeight: number;      // multiple de la taille
  align: TextAlign;
  uppercase: boolean;

  // Remplissage
  fillMode: FillMode;
  color: string;
  colorAlpha: number;
  stops: ColorStop[];
  gradientAngle: number;   // degrés

  // Contour
  strokeWidth: number;     // % de la taille de police
  strokeColor: string;
  strokeAlpha: number;

  // Ombre
  shadow: boolean;
  shadowColor: string;
  shadowAlpha: number;
  shadowBlur: number;      // % de la taille
  shadowX: number;         // % de la taille
  shadowY: number;

  // Logo
  logoPath: string | null;

  // Placement
  size: number;            // % du plus petit côté (hauteur de police ; logo = 4x en largeur)
  x: number;               // centre, 0–1
  y: number;
  rotation: number;        // degrés
  opacity: number;         // 0–1
  flipH: boolean;
  flipV: boolean;

  // Mosaïque
  tile: boolean;
  tileGapX: number;        // % du plus petit côté
  tileGapY: number;
  tileStagger: boolean;

  // Audio (tatouage sonore)
  audioSound: string | null; // son mixé dans la piste
  audioInterval: number;     // secondes, 0 = une seule fois
  audioOffset: number;       // secondes
  audioVolume: number;       // 0–2
}

export const DEFAULT_CONFIG: WatermarkConfig = {
  kind: "text",
  text: "CONFIDENTIEL",
  fontFamily: "Arial",
  fontFile: null,
  bold: true,
  italic: false,
  letterSpacing: 0.05,
  lineHeight: 1.2,
  align: "center",
  uppercase: false,

  fillMode: "solid",
  color: "#ffffff",
  colorAlpha: 1,
  stops: [
    { offset: 0, color: "#3b82f6", alpha: 1 },
    { offset: 1, color: "#ec4899", alpha: 1 },
  ],
  gradientAngle: 0,

  strokeWidth: 0,
  strokeColor: "#000000",
  strokeAlpha: 1,

  shadow: false,
  shadowColor: "#000000",
  shadowAlpha: 0.6,
  shadowBlur: 8,
  shadowX: 3,
  shadowY: 3,

  logoPath: null,

  size: 10,
  x: 0.5,
  y: 0.5,
  rotation: -30,
  opacity: 0.45,
  flipH: false,
  flipV: false,

  tile: false,
  tileGapX: 12,
  tileGapY: 14,
  tileStagger: true,

  audioSound: null,
  audioInterval: 30,
  audioOffset: 5,
  audioVolume: 0.6,
};

// ── Préréglages ──────────────────────────────────────────────────────────────

export interface Preset {
  name: string;
  config: WatermarkConfig;
  builtin?: boolean;
}

export const BUILTIN_PRESETS: Preset[] = [
  { name: "Confidentiel diagonal", builtin: true, config: DEFAULT_CONFIG },
  {
    name: "Copyright bas-droite", builtin: true,
    config: {
      ...DEFAULT_CONFIG, text: "© Mon nom 2026", bold: false, size: 4, rotation: 0,
      x: 0.82, y: 0.93, opacity: 0.8, shadow: true, letterSpacing: 0,
    },
  },
  {
    name: "Mosaïque dégradée", builtin: true,
    config: {
      ...DEFAULT_CONFIG, text: "ÉCHANTILLON", size: 5, opacity: 0.3, tile: true,
      fillMode: "linear",
    },
  },
  {
    name: "Contour seul", builtin: true,
    config: {
      ...DEFAULT_CONFIG, text: "BROUILLON", size: 16, fillMode: "none",
      strokeWidth: 3, strokeColor: "#ff2d55", opacity: 0.6,
    },
  },
];

const PRESETS_KEY = "uc_watermark_presets";
const LAST_KEY = "uc_watermark_last";

export function loadUserPresets(): Preset[] {
  try {
    const raw = JSON.parse(localStorage.getItem(PRESETS_KEY) ?? "[]") as Preset[];
    return raw.map((p) => ({ name: p.name, config: { ...DEFAULT_CONFIG, ...p.config } }));
  } catch {
    return [];
  }
}

export function saveUserPresets(presets: Preset[]): boolean {
  try {
    localStorage.setItem(PRESETS_KEY, JSON.stringify(presets));
    return true;
  } catch {
    return false;
  }
}

export function loadLastConfig(): WatermarkConfig {
  try {
    const raw = localStorage.getItem(LAST_KEY);
    return raw ? { ...DEFAULT_CONFIG, ...JSON.parse(raw) } : DEFAULT_CONFIG;
  } catch {
    return DEFAULT_CONFIG;
  }
}

export function saveLastConfig(cfg: WatermarkConfig) {
  try { localStorage.setItem(LAST_KEY, JSON.stringify(cfg)); } catch { /* stockage indisponible : sans conséquence */ }
}

// ── Polices ─────────────────────────────────────────────────────────────────

export const COMMON_FONTS = [
  "Arial", "Arial Black", "Bahnschrift", "Calibri", "Cambria", "Candara",
  "Comic Sans MS", "Consolas", "Constantia", "Corbel", "Courier New",
  "Franklin Gothic Medium", "Gabriola", "Georgia", "Impact", "Ink Free",
  "Lucida Console", "Lucida Sans Unicode", "Palatino Linotype", "Segoe Print",
  "Segoe Script", "Segoe UI", "Segoe UI Black", "Sitka Text", "Tahoma",
  "Times New Roman", "Trebuchet MS", "Verdana",
];

export function rgba(hex: string, alpha: number): string {
  const h = hex.match(/^#?([0-9a-f]{6})$/i)?.[1] ?? "000000";
  const n = parseInt(h, 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${alpha})`;
}
