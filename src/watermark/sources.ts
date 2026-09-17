// Chargement des fichiers à filigraner. Chaque format garde son format :
// seule l'option « produire un PDF » change cela, pour les formats qui ne
// peuvent porter aucun filigrane (CSV, JSON, anciens DOC/PPT/XLS).

import { invoke } from "@tauri-apps/api/core";
import { tempDir } from "@tauri-apps/api/path";
import * as pdfjsLib from "pdfjs-dist";
import type { PDFDocumentProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { IMAGE_EXTENSIONS, ConversionResult } from "../types";

pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

/** Côté maximal du rendu d'aperçu d'une page PDF. */
const PREVIEW_SIDE = 1600;
/** Résolution du calque exporté pour un PDF ou un document paginé. */
export const PAGE_OVERLAY_DPI = 200;

export interface StudioFile {
  id: string;
  path: string;
  name: string;
  ext: string;
}

export interface PageSize {
  w: number; // points, orientation affichée
  h: number;
}

/**
 * - image     : raster, recomposé au même format
 * - svg       : calque intégré au SVG
 * - pdf       : calque tamponné sur les pages
 * - visualDoc : DOCX, PPTX, XLSX, ODT, ODS, ODP, EPUB, HTML — calque intégré, format conservé
 * - textDoc   : TXT, MD, YAML, TOML, XML, RTF — texte inscrit dans le format
 * - subtitle  : SRT, VTT — cue permanent
 * - audio     : tatouage sonore
 * - archive   : contenu traité puis réemballé
 * - pdfOnly   : CSV, JSON, DOC, PPT, XLS — seulement via l'option PDF
 */
export type Family = "image" | "svg" | "pdf" | "visualDoc" | "textDoc" | "subtitle" | "audio" | "archive" | "pdfOnly";

const VISUAL_DOCS = new Set(["docx", "pptx", "xlsx", "odt", "ods", "odp", "epub", "html", "htm"]);
const TEXT_DOCS = new Set(["txt", "md", "markdown", "yaml", "yml", "toml", "xml", "rtf"]);
const SUBTITLES = new Set(["srt", "vtt"]);
const AUDIO = new Set(["mp3", "ogg", "m4a", "aac", "wav", "flac"]);
const ARCHIVES = new Set(["zip", "tar", "tgz", "gz", "7z"]);
/** Documents paginés : calque rendu en haute définition. Les autres (écran, tuile Excel) en 1:1. */
export const PAGED_DOCS = new Set(["docx", "pptx", "odt", "ods", "odp"]);

export function familyOf(ext: string): Family {
  if (ext === "svg") return "svg";
  if (IMAGE_EXTENSIONS.has(ext)) return "image";
  if (ext === "pdf") return "pdf";
  if (VISUAL_DOCS.has(ext)) return "visualDoc";
  if (TEXT_DOCS.has(ext)) return "textDoc";
  if (SUBTITLES.has(ext)) return "subtitle";
  if (AUDIO.has(ext)) return "audio";
  if (ARCHIVES.has(ext)) return "archive";
  return "pdfOnly";
}

/** Familles dont l'aperçu dessine le filigrane visuel complet. */
export const usesVisualMark = (f: Family) => ["image", "svg", "pdf", "visualDoc", "pdfOnly"].includes(f);

export interface ArchiveEntry { name: string; path: string }

export type Loaded =
  | { kind: "image"; path: string; bg: HTMLImageElement; width: number; height: number }
  | { kind: "pdf"; path: string; doc: PDFDocumentProxy; sizes: PageSize[] }
  | { kind: "frame"; path: string; bg: HTMLCanvasElement; family: "subtitle" | "textDoc" }
  | { kind: "audio"; path: string }
  | { kind: "archive"; path: string; entries: ArchiveEntry[]; inner: Loaded | null; innerName: string | null };

export function studioFile(path: string): StudioFile {
  const name = path.split(/[\\/]/).pop() ?? path;
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return { id: `${path}-${Math.random()}`, path, name, ext };
}

export function loadImageElement(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Image illisible"));
    img.src = src;
  });
}

async function previewDataUrl(path: string) {
  return invoke<{ dataUrl: string; width: number; height: number }>("watermark_preview", { inputPath: path });
}

/** Logo : n'importe quel format image lu par le moteur Rust (SVG, TIFF, AVIF…). */
export async function loadLogo(path: string): Promise<HTMLImageElement> {
  return loadImageElement((await previewDataUrl(path)).dataUrl);
}

/** Police importée : enregistrée sous son nom de fichier et chargée. */
export async function loadFontFile(path: string): Promise<string> {
  const family = `UC ${(path.split(/[\\/]/).pop() ?? "police").replace(/\.[^.]+$/, "")}`;
  if ([...document.fonts].some((f) => f.family === family)) return family;
  const bytes = await invoke<ArrayBuffer>("read_watermark_asset", { path });
  const face = new FontFace(family, bytes);
  await face.load();
  document.fonts.add(face);
  return family;
}

/** Libère les ressources pdf.js d'un fichier chargé (y compris dans une archive). */
export function disposeLoaded(l: Loaded | null) {
  if (!l) return;
  if (l.kind === "pdf") void l.doc.destroy();
  if (l.kind === "archive") disposeLoaded(l.inner);
}

/** Cadre d'aperçu 16:9 pour les sous-titres et les formats texte. */
function frame(label: string): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = 1600;
  c.height = 900;
  const ctx = c.getContext("2d")!;
  const g = ctx.createLinearGradient(0, 0, 0, 900);
  g.addColorStop(0, "#1e293b");
  g.addColorStop(1, "#020617");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, 1600, 900);
  ctx.font = "40px sans-serif";
  ctx.textAlign = "center";
  ctx.fillStyle = "rgba(255,255,255,0.3)";
  ctx.fillText(label, 800, 840);
  return c;
}

/** Convertit en PDF temporaire (aperçu d'un document, ou option PDF). */
export async function convertToTempPdf(file: StudioFile): Promise<string> {
  const formats = await invoke<string[]>("get_formats_for_extension", { inputExt: file.ext }).catch((): string[] => []);
  if (!formats.includes("pdf")) throw new Error(`Aucun aperçu possible pour .${file.ext}`);
  const res = await invoke<ConversionResult>("convert_file", {
    inputPath: file.path,
    outputFormat: "pdf",
    outputDir: await tempDir(),
    outputName: `uc_filigrane_${Date.now()}_${Math.floor(Math.random() * 1e6)}`,
    quality: null, resizeWidth: null, resizeHeight: null, rotation: null,
  });
  return res.path;
}

async function loadPdf(pdfPath: string): Promise<Loaded> {
  const bytes = await invoke<ArrayBuffer>("read_watermark_asset", { path: pdfPath });
  const doc = await pdfjsLib.getDocument({ data: new Uint8Array(bytes) }).promise;
  const sizes: PageSize[] = [];
  for (let i = 1; i <= doc.numPages; i++) {
    const vp = (await doc.getPage(i)).getViewport({ scale: 1 });
    sizes.push({ w: vp.width, h: vp.height });
  }
  return { kind: "pdf", path: pdfPath, doc, sizes };
}

/** Charge l'aperçu d'un fichier selon sa famille. */
export async function loadSource(file: StudioFile): Promise<Loaded> {
  switch (familyOf(file.ext)) {
    case "image":
    case "svg": {
      const p = await previewDataUrl(file.path);
      return { kind: "image", path: file.path, bg: await loadImageElement(p.dataUrl), width: p.width, height: p.height };
    }
    case "pdf":
      return loadPdf(file.path);
    case "visualDoc":
    case "pdfOnly":
      // Aperçu du contenu via une conversion PDF ; la sortie, elle, garde le format.
      return loadPdf(await convertToTempPdf(file));
    case "textDoc":
      return { kind: "frame", path: file.path, bg: frame("Le texte du filigrane sera inscrit dans le fichier"), family: "textDoc" };
    case "subtitle":
      return { kind: "frame", path: file.path, bg: frame("Sous-titre existant"), family: "subtitle" };
    case "audio":
      return { kind: "audio", path: file.path };
    case "archive": {
      const entries = await invoke<ArchiveEntry[]>("watermark_archive_extract", { inputPath: file.path });
      const first = entries.map((e) => studioFile(e.path)).find((f) => familyOf(f.ext) !== "audio" && familyOf(f.ext) !== "archive");
      const inner = first ? await loadSource(first).catch(() => null) : null;
      return { kind: "archive", path: file.path, entries, inner, innerName: first?.name ?? null };
    }
  }
}

/** Rend une page PDF (1-based) dans un canvas d'aperçu. */
export async function renderPdfPage(doc: PDFDocumentProxy, n: number): Promise<HTMLCanvasElement> {
  const page = await doc.getPage(n);
  const base = page.getViewport({ scale: 1 });
  const scale = Math.min(4, PREVIEW_SIDE / Math.max(base.width, base.height));
  const vp = page.getViewport({ scale });
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(vp.width);
  canvas.height = Math.round(vp.height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas 2D indisponible");
  await page.render({ canvasContext: ctx, viewport: vp }).promise;
  return canvas;
}

/** Regroupe les pages ciblées par format : un calque par format de page. */
export function groupPagesBySize(sizes: PageSize[], pages: number[]) {
  const groups = new Map<string, { size: PageSize; pages: number[] }>();
  for (const n of pages) {
    const s = sizes[n - 1];
    const key = `${Math.round(s.w)}x${Math.round(s.h)}`;
    const g = groups.get(key) ?? { size: s, pages: [] };
    g.pages.push(n);
    groups.set(key, g);
  }
  return [...groups.values()].map((g) => ({
    ...g,
    px: { w: (g.size.w * PAGE_OVERLAY_DPI) / 72, h: (g.size.h * PAGE_OVERLAY_DPI) / 72 },
  }));
}
