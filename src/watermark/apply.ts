// Application du filigrane à un fichier, en conservant son format.
// Une archive rappelle cette même fonction pour chacun de ses fichiers.

import { invoke } from "@tauri-apps/api/core";
import { WatermarkConfig } from "./config";
import { renderOverlay } from "./render";
import {
  StudioFile, Loaded, studioFile, loadSource, groupPagesBySize, familyOf, disposeLoaded,
  convertToTempPdf, PAGED_DOCS, PAGE_OVERLAY_DPI,
} from "./sources";
import { ConversionResult, parsePageRange } from "../types";

/** Formats image que le moteur Rust sait écrire. */
export const OUTPUT_IMAGE_FORMATS = ["png", "jpg", "webp", "bmp", "gif", "tiff", "tga", "ico", "qoi", "avif", "exr", "ppm", "ff"];
const EXT_ALIASES: Record<string, string> = { jpeg: "jpg", tif: "tiff", pnm: "ppm" };
/** 7z et gz n'ont pas d'encodeur : réemballés en zip / tgz. */
const ARCHIVE_OUT: Record<string, string> = { zip: "zip", tar: "tar", tgz: "tgz", gz: "tgz", "7z": "zip" };

/**
 * Format de sortie « d'origine » d'une image. HDR et DDS n'ont pas d'encodeur
 * dans le moteur : ce sont les deux seuls cas écrits en PNG.
 */
export function sameImageFormat(ext: string): string {
  const e = EXT_ALIASES[ext] ?? ext;
  return OUTPUT_IMAGE_FORMATS.includes(e) ? e : "png";
}

export interface ApplyOptions {
  cfg: WatermarkConfig;
  logo: HTMLImageElement | null;
  imageFormat: string;   // "same" ou un format imposé
  audioFormat: "wav" | "flac";
  quality: number;
  pageRange: string;     // "" = toutes (PDF)
  pdfFallback: boolean;  // CSV, JSON, DOC, PPT, XLS : produire un PDF filigrané
  outputDir: string | null;
  outputName?: string | null;
}

const dirOf = (p: string) => p.split(/[\\/]/).slice(0, -1).join("\\");
const stemOf = (name: string) => name.replace(/\.[^.]+$/, "");

/** Raison pour laquelle un fichier ne peut pas être traité avec ces réglages, sinon null. */
export function missingRequirement(f: StudioFile, o: ApplyOptions): string | null {
  const fam = familyOf(f.ext);
  if (fam === "audio" && !o.cfg.audioSound) return "Choisissez le son du tatouage sonore";
  if (fam === "pdfOnly" && !o.pdfFallback) {
    return `.${f.ext} ne peut porter aucun filigrane sans altérer ses données : activez « produire un PDF » pour ce format`;
  }
  if (["image", "svg", "pdf", "visualDoc", "pdfOnly"].includes(fam) && o.cfg.kind === "image" && !o.logo) {
    return "Choisissez d'abord une image pour le filigrane";
  }
  if (["textDoc", "subtitle"].includes(fam) && !o.cfg.text.trim()) return "Le texte du filigrane est vide";
  return null;
}

export async function watermarkFile(f: StudioFile, o: ApplyOptions, preloaded?: Loaded | null): Promise<ConversionResult> {
  const missing = missingRequirement(f, o);
  if (missing) throw new Error(missing);
  const outputName = o.outputName ?? null;
  const base = { outputDir: o.outputDir, outputName };

  switch (familyOf(f.ext)) {
    case "subtitle":
      return invoke("watermark_subtitles_command", { inputPath: f.path, text: o.cfg.text, x: o.cfg.x, y: o.cfg.y, ...base });

    case "textDoc":
      return invoke("watermark_text_command", { inputPath: f.path, text: o.cfg.text, ...base });

    case "audio":
      return invoke("watermark_audio_command", {
        inputPath: f.path, soundPath: o.cfg.audioSound,
        intervalS: o.cfg.audioInterval, offsetS: o.cfg.audioOffset, volume: o.cfg.audioVolume,
        outputFormat: o.audioFormat, ...base,
      });

    case "archive":
      return watermarkArchive(f, o, preloaded);

    case "image":
    case "svg": {
      const src = preloaded?.kind === "image" ? preloaded : await loadSource(f);
      if (src.kind !== "image") throw new Error(`Aperçu inattendu pour .${f.ext}`);
      const fmt = f.ext === "svg" ? "svg" : o.imageFormat === "same" ? sameImageFormat(f.ext) : o.imageFormat;
      return invoke("apply_watermark", {
        inputPath: f.path,
        layers: [{ overlay: renderOverlay(src.width, src.height, o.cfg, o.logo), pages: [] }],
        outputFormat: fmt, quality: o.quality,
        blend: o.cfg.blend,
        // Signature invisible : seules les images raster la portent.
        signature: f.ext === "svg" ? null : o.cfg.signature || null,
        ...base,
      });
    }

    case "visualDoc": {
      const size = await invoke<{ width: number; height: number }>("watermark_doc_size", { inputPath: f.path });
      const k = PAGED_DOCS.has(f.ext) ? PAGE_OVERLAY_DPI / 72 : 1;
      return invoke("apply_watermark", {
        inputPath: f.path,
        layers: [{ overlay: renderOverlay(size.width * k, size.height * k, o.cfg, o.logo), pages: [] }],
        outputFormat: f.ext, quality: null, blend: "normal", signature: null, ...base,
      });
    }

    case "pdf":
    case "pdfOnly": {
      const converted = f.ext !== "pdf";
      let src = preloaded?.kind === "pdf" ? preloaded : null;
      const owned = !src;
      if (!src) {
        const loaded = await loadSource(converted ? { ...f, path: await convertToTempPdf(f), ext: "pdf" } : f);
        if (loaded.kind !== "pdf") throw new Error("PDF illisible");
        src = loaded;
      }
      try {
        const total = src.sizes.length;
        const pages = o.pageRange.trim() ? parsePageRange(o.pageRange, total) : Array.from({ length: total }, (_, k) => k + 1);
        const layers = groupPagesBySize(src.sizes, pages).map((g) => ({
          overlay: renderOverlay(g.px.w, g.px.h, o.cfg, o.logo),
          pages: g.pages,
        }));
        return await invoke("apply_watermark", {
          inputPath: src.path,
          layers,
          outputFormat: "pdf",
          quality: null,
          blend: o.cfg.blend,
          signature: null, // un PDF n'est pas une image : pas de signature fréquentielle

          // Document converti : le PDF source est temporaire, la sortie va à côté de l'original.
          outputDir: o.outputDir ?? (converted ? dirOf(f.path) : null),
          outputName: outputName ?? (converted ? `${stemOf(f.name)}_filigrane` : null),
        });
      } finally {
        if (owned) disposeLoaded(src);
      }
    }
  }
}

/**
 * Archive : chaque fichier traitable l'est dans le dossier temporaire
 * d'extraction avec son format d'origine ; le reste est recopié tel quel.
 * ponytail: une archive dans l'archive est recopiée sans être ouverte ;
 * rappeler watermarkArchive sur ses entrées si ce cas se présente.
 */
async function watermarkArchive(f: StudioFile, o: ApplyOptions, preloaded?: Loaded | null): Promise<ConversionResult> {
  const entries = preloaded?.kind === "archive"
    ? preloaded.entries
    : await invoke<{ name: string; path: string }[]>("watermark_archive_extract", { inputPath: f.path });

  const packed: { name: string; path: string }[] = [];
  let marked = 0;
  for (const e of entries) {
    const inner = studioFile(e.path);
    if (!inner.ext || familyOf(inner.ext) === "archive" || missingRequirement(inner, o)) {
      packed.push(e);
      continue;
    }
    try {
      const res = await watermarkFile(inner, {
        ...o,
        imageFormat: "same",
        pageRange: "",
        outputDir: dirOf(e.path),
        outputName: `${stemOf(inner.name)}.uc-filigrane`,
      });
      // Nom et dossier d'origine conservés ; seule l'extension suit la sortie (mp3 -> wav, csv -> pdf).
      const outExt = res.path.split(".").pop() ?? inner.ext;
      packed.push({ name: e.name.replace(/[^/]+$/, `${stemOf(inner.name)}.${outExt}`), path: res.path });
      marked++;
    } catch {
      packed.push(e);
    }
  }
  if (marked === 0) throw new Error("Aucun fichier filigranable dans l'archive");

  return invoke("watermark_archive_pack", {
    inputPath: f.path,
    entries: packed,
    outputFormat: ARCHIVE_OUT[f.ext] ?? "zip",
    outputDir: o.outputDir,
    outputName: o.outputName ?? null,
  });
}
