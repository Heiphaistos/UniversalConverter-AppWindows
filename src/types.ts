export type ConversionStatus = "idle" | "converting" | "done" | "error";
export type Rotation = 0 | 90 | 180 | 270;

export interface ImageOptions {
  quality: number;       // 1–100
  resizeWidth: string;   // "" ou nombre
  resizeHeight: string;
  rotation: Rotation;
}

export interface FileItem {
  id: string;
  name: string;
  path: string;
  extension: string;
  availableFormats: string[];
  selectedFormat: string;
  status: ConversionStatus;
  progress: number;
  outputPath?: string;
  errorMessage?: string;
  // Métadonnées
  fileSize?: number;
  outputSize?: number;
  thumbnail?: string;
  pageCount?: number;
  // Options de conversion image
  imageOptions: ImageOptions;
  outputName: string;
  showOptions: boolean;
}

export interface ConversionResult {
  path: string;
  outputSize: number;
}

export interface HistoryItem {
  id: string;
  inputName: string;
  inputExt: string;
  outputPath: string;
  outputFormat: string;
  outputSize: number;
  timestamp: number;
}

export function defaultImageOptions(): ImageOptions {
  return { quality: 90, resizeWidth: "", resizeHeight: "", rotation: 0 };
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function parsePageRange(input: string, total: number): number[] {
  const pages = new Set<number>();
  for (const part of input.split(",")) {
    const t = part.trim();
    if (!t) continue;

    if (t.includes("-")) {
      const segments = t.split("-");
      if (segments.length !== 2) throw new Error(`Plage invalide: "${t}"`);
      const a = parseInt(segments[0].trim(), 10);
      const b = parseInt(segments[1].trim(), 10);
      if (isNaN(a) || isNaN(b)) throw new Error(`Plage invalide: "${t}"`);
      if (a > b) throw new Error(`Plage inversée: "${t}" (début > fin)`);
      if (a < 1 || b > total) throw new Error(`Pages hors limites: "${t}" (1–${total})`);
      for (let i = a; i <= b; i++) pages.add(i);
    } else {
      const n = parseInt(t, 10);
      if (isNaN(n) || String(n) !== t) throw new Error(`Numéro invalide: "${t}"`);
      if (n < 1 || n > total) throw new Error(`Page ${n} hors limites (1–${total})`);
      pages.add(n);
    }
  }
  if (pages.size === 0) throw new Error("Aucune page sélectionnée");
  return Array.from(pages).sort((a, b) => a - b);
}

export const FORMAT_LABELS: Record<string, string> = {
  png: "PNG", jpg: "JPEG", webp: "WebP", bmp: "BMP", gif: "GIF",
  tiff: "TIFF", ico: "ICO", tga: "TGA", pnm: "PNM", hdr: "HDR", avif: "AVIF",
  qoi: "QOI", exr: "EXR", dds: "DDS", ppm: "PPM", ff: "Farbfeld",
  svg: "SVG",
  pdf: "PDF", txt: "TXT", html: "HTML", md: "Markdown", rtf: "RTF", epub: "EPUB",
  docx: "Word", doc: "Word (Legacy)", pptx: "PowerPoint", ppt: "PowerPoint (Legacy)",
  odt: "ODT (Writer)", odp: "ODP (Impress)",
  xlsx: "Excel", xls: "Excel (Legacy)", ods: "ODS",
  csv: "CSV", json: "JSON", yaml: "YAML", yml: "YAML", toml: "TOML", xml: "XML",
  srt: "SRT", vtt: "WebVTT",
  zip: "ZIP", tar: "TAR", tgz: "TAR.GZ", gz: "GZ", "7z": "7-Zip",
  mp3: "MP3", wav: "WAV", flac: "FLAC", ogg: "OGG", m4a: "M4A", aac: "AAC",
};

export const IMAGE_EXTENSIONS = new Set([
  "png","jpg","jpeg","webp","bmp","gif","tiff","tif","tga","pnm","ppm","hdr","ico","svg",
  "qoi","exr","dds","ff",
]);
