// Moteur de rendu unique du filigrane : sert à l'aperçu ET au calque exporté.
// Aucune autre implémentation n'existe, l'aperçu ne peut donc pas mentir.

import { WatermarkConfig, rgba } from "./config";

/** Bloc du filigrane central, en pixels canvas (taille avant rotation). */
export interface Box {
  cx: number;
  cy: number;
  w: number;
  h: number;
  angle: number; // radians
}

export function fontCss(cfg: WatermarkConfig, px: number): string {
  return `${cfg.italic ? "italic " : ""}${cfg.bold ? "700" : "400"} ${px}px "${cfg.fontFamily}", sans-serif`;
}

function lines(cfg: WatermarkConfig): string[] {
  const t = cfg.uppercase ? cfg.text.toUpperCase() : cfg.text;
  return t.split("\n");
}

/** Mesure le bloc sans rien dessiner. */
export function measure(
  ctx: CanvasRenderingContext2D,
  W: number,
  H: number,
  cfg: WatermarkConfig,
  logo: HTMLImageElement | null,
): Box {
  const ref = Math.min(W, H);
  const px = (cfg.size / 100) * ref;
  let w = 0;
  let h = 0;

  if (cfg.kind === "image") {
    if (logo && logo.naturalWidth > 0) {
      w = px * 4; // un logo « taille 10 » occupe 40 % du petit côté
      h = (w * logo.naturalHeight) / logo.naturalWidth;
    }
  } else {
    ctx.save();
    ctx.font = fontCss(cfg, px);
    ctx.letterSpacing = `${cfg.letterSpacing * px}px`;
    const ls = lines(cfg);
    w = Math.max(0, ...ls.map((l) => ctx.measureText(l).width));
    h = ls.length * px * cfg.lineHeight;
    ctx.restore();
  }

  return { cx: cfg.x * W, cy: cfg.y * H, w, h, angle: (cfg.rotation * Math.PI) / 180 };
}

/** Dessine le filigrane (bloc unique ou mosaïque) et renvoie le bloc central. */
export function drawWatermark(
  ctx: CanvasRenderingContext2D,
  W: number,
  H: number,
  cfg: WatermarkConfig,
  logo: HTMLImageElement | null,
): Box {
  const box = measure(ctx, W, H, cfg, logo);
  if (box.w <= 0 || box.h <= 0) return box;

  const ref = Math.min(W, H);
  const px = (cfg.size / 100) * ref;
  const ls = lines(cfg);
  const lineH = px * cfg.lineHeight;

  ctx.save();
  ctx.globalAlpha = cfg.opacity;
  ctx.translate(box.cx, box.cy);
  ctx.rotate(box.angle);
  ctx.scale(cfg.flipH ? -1 : 1, cfg.flipV ? -1 : 1);

  // Styles préparés une fois : un dégradé est exprimé dans le repère du bloc
  // et suit chaque translation de la mosaïque.
  let fill: string | CanvasGradient | null = null;
  if (cfg.kind === "text") {
    ctx.font = fontCss(cfg, px);
    ctx.letterSpacing = `${cfg.letterSpacing * px}px`;
    ctx.textBaseline = "middle";
    ctx.textAlign = cfg.align;
    ctx.lineJoin = "round";
    ctx.miterLimit = 2;
    fill = makeFill(ctx, cfg, box.w, box.h);
  }

  const drawBlock = () => {
    if (cfg.shadow) {
      ctx.shadowColor = rgba(cfg.shadowColor, cfg.shadowAlpha);
      ctx.shadowBlur = (cfg.shadowBlur / 100) * px;
      ctx.shadowOffsetX = (cfg.shadowX / 100) * px;
      ctx.shadowOffsetY = (cfg.shadowY / 100) * px;
    }

    if (cfg.kind === "image") {
      if (logo) ctx.drawImage(logo, -box.w / 2, -box.h / 2, box.w, box.h);
      resetShadow(ctx);
      return;
    }

    const x = cfg.align === "left" ? -box.w / 2 : cfg.align === "right" ? box.w / 2 : 0;
    const stroke = (cfg.strokeWidth / 100) * px;
    ls.forEach((line, i) => {
      const y = -box.h / 2 + lineH * (i + 0.5);
      // Contour d'abord, remplissage par-dessus : le trait reste à l'extérieur.
      if (stroke > 0) {
        ctx.lineWidth = fill ? stroke * 2 : stroke;
        ctx.strokeStyle = rgba(cfg.strokeColor, cfg.strokeAlpha);
        ctx.strokeText(line, x, y);
        if (fill) resetShadow(ctx); // une seule ombre par ligne
      }
      if (fill) {
        ctx.fillStyle = fill;
        ctx.fillText(line, x, y);
      }
    });
    resetShadow(ctx);
  };

  if (!cfg.tile) {
    drawBlock();
  } else {
    const stepX = box.w + (cfg.tileGapX / 100) * ref;
    const stepY = box.h + (cfg.tileGapY / 100) * ref;
    // Couvrir la diagonale : la grille tourne avec le bloc.
    const diag = Math.hypot(W, H);
    const nx = Math.ceil(diag / stepX) + 1;
    const ny = Math.ceil(diag / stepY) + 1;
    for (let j = -ny; j <= ny; j++) {
      const off = cfg.tileStagger && j % 2 !== 0 ? stepX / 2 : 0;
      for (let i = -nx; i <= nx; i++) {
        ctx.save();
        ctx.translate(i * stepX + off, j * stepY);
        drawBlock();
        ctx.restore();
      }
    }
  }

  ctx.restore();
  return box;
}

function resetShadow(ctx: CanvasRenderingContext2D) {
  ctx.shadowColor = "transparent";
  ctx.shadowBlur = 0;
  ctx.shadowOffsetX = 0;
  ctx.shadowOffsetY = 0;
}

function makeFill(
  ctx: CanvasRenderingContext2D,
  cfg: WatermarkConfig,
  w: number,
  h: number,
): string | CanvasGradient | null {
  if (cfg.fillMode === "none") return null;
  if (cfg.fillMode === "solid" || cfg.stops.length < 2) return rgba(cfg.color, cfg.colorAlpha);

  let g: CanvasGradient;
  if (cfg.fillMode === "radial") {
    g = ctx.createRadialGradient(0, 0, 0, 0, 0, Math.hypot(w, h) / 2);
  } else {
    const a = (cfg.gradientAngle * Math.PI) / 180;
    const [c, s] = [Math.cos(a), Math.sin(a)];
    // Demi-projection du bloc sur l'axe : le dégradé va d'un bord à l'autre.
    const half = Math.abs((w / 2) * c) + Math.abs((h / 2) * s);
    g = ctx.createLinearGradient(-c * half, -s * half, c * half, s * half);
  }
  [...cfg.stops]
    .sort((p, q) => p.offset - q.offset)
    .forEach((st) => g.addColorStop(Math.min(1, Math.max(0, st.offset)), rgba(st.color, st.alpha)));
  return g;
}

/** Côté maximal d'un calque exporté ; au-delà, Rust l'agrandit (léger flou). */
export const MAX_OVERLAY_SIDE = 8192;

/**
 * Calque transparent aux dimensions de sortie, prêt à envoyer à Rust.
 * ponytail: plafonné à 8192 px de côté (mémoire du webview) ; au-delà le
 * calque est agrandi côté Rust. Découper en tuiles si des images > 8K l'exigent.
 */
export function renderOverlay(
  width: number,
  height: number,
  cfg: WatermarkConfig,
  logo: HTMLImageElement | null,
): string {
  const k = Math.min(1, MAX_OVERLAY_SIDE / Math.max(width, height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width * k));
  canvas.height = Math.max(1, Math.round(height * k));
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas 2D indisponible");
  drawWatermark(ctx, canvas.width, canvas.height, cfg, logo);
  return canvas.toDataURL("image/png");
}
