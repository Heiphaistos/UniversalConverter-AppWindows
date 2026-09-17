// Studio de filigrane : aperçu temps réel, placement à la souris, application
// par lot à tous les formats d'UniversalConverter, format d'origine conservé.

import { useCallback, useEffect, useRef, useState, PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { WatermarkConfig, Preset, loadLastConfig, saveLastConfig, loadUserPresets, saveUserPresets, DEFAULT_CONFIG } from "./config";
import { renderLayer, Box } from "./render";
import {
  StudioFile, Loaded, studioFile, loadSource, renderPdfPage, loadLogo, loadFontFile, familyOf, disposeLoaded, Family,
} from "./sources";
import { watermarkFile, ApplyOptions, OUTPUT_IMAGE_FORMATS } from "./apply";
import { WatermarkPanel } from "./WatermarkPanel";
import { btn, Slider } from "./controls";
import { HistoryItem, FORMAT_LABELS, formatBytes } from "../types";

interface Props {
  initialPaths: string[];
  outputDir: string | null;
  onClose: () => void;
  onResults: (items: HistoryItem[]) => void;
}

type DragMode = "move" | "scale" | "rotate";
interface Drag { mode: DragMode; cfg0: WatermarkConfig; nx0: number; ny0: number; d0: number }
interface Result { name: string; path?: string; size?: number; error?: string }

const SNAP = 0.012;
const MAX_UNDO = 100;

/** Ce que le studio fera du fichier affiché, en une phrase. */
const FAMILY_INFO: Record<Family, string> = {
  image: "Image : filigrane incrusté, format d'origine conservé.",
  svg: "SVG : filigrane intégré au fichier, qui reste un SVG.",
  pdf: "PDF : filigrane posé sur les pages choisies.",
  visualDoc: "Document : filigrane intégré au fichier, qui garde son format. Aperçu du contenu à titre indicatif.",
  textDoc: "Texte pur : aucune image possible, le texte du filigrane est inscrit dans le fichier (même format).",
  subtitle: "Sous-titres : le texte du filigrane devient un sous-titre permanent, placé selon la position choisie. Police et couleur : celles du lecteur vidéo.",
  audio: "Audio : tatouage sonore, votre son mixé à intervalle régulier.",
  archive: "Archive : chaque fichier compatible est filigrané dans son format, le reste est recopié.",
  pdfOnly: "Ce format ne peut porter aucun filigrane sans altérer ses données : sortie en PDF uniquement si vous l'autorisez.",
};

/** Sous-titres : aperçu approximatif, rendu type lecteur vidéo. */
function drawSubtitlePreview(ctx: CanvasRenderingContext2D, W: number, H: number, cfg: WatermarkConfig) {
  const lines = cfg.text.split("\n").filter((l) => l.trim());
  const px = H * 0.05;
  ctx.font = `600 ${px}px sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.lineWidth = px * 0.18;
  ctx.strokeStyle = "rgba(0,0,0,0.9)";
  ctx.fillStyle = "#fff";
  lines.forEach((l, i) => {
    const y = cfg.y * H + (i - (lines.length - 1) / 2) * px * 1.25;
    ctx.strokeText(l, cfg.x * W, y);
    ctx.fillText(l, cfg.x * W, y);
  });
}

/** Formats texte : le texte tel qu'il sera inscrit. */
function drawTextDocPreview(ctx: CanvasRenderingContext2D, W: number, H: number, cfg: WatermarkConfig) {
  const one = cfg.text.split("\n").map((l) => l.trim()).filter(Boolean).join(" · ");
  ctx.font = `${H * 0.035}px Consolas, monospace`;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  ctx.fillStyle = "#fbbf24";
  ctx.fillText(one, W * 0.05, H * 0.08);
  ctx.fillStyle = "rgba(255,255,255,0.35)";
  ["…contenu d'origine inchangé…", "", one].forEach((l, i) => ctx.fillText(l, W * 0.05, H * (0.2 + i * 0.07)));
}

/**
 * Zone la plus « chaotique » de l'image : énergie des contours (Sobel) calculée
 * sur l'aperçu, puis fenêtre de la taille du filigrane qui en contient le plus.
 * Un filigrane posé là est bien plus coûteux à effacer proprement qu'au milieu
 * d'un ciel uni, où un simple remplissage suffit.
 */
function busiestSpot(canvas: HTMLCanvasElement, boxW: number, boxH: number): { x: number; y: number } | null {
  const W = canvas.width;
  const H = canvas.height;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx || W < 8 || H < 8) return null;
  const d = ctx.getImageData(0, 0, W, H).data;

  // Carte d'énergie en petites cellules : assez fin pour viser, assez rapide pour l'interactif.
  const cell = Math.max(4, Math.round(Math.min(W, H) / 120));
  const cw = Math.floor(W / cell);
  const ch = Math.floor(H / cell);
  if (cw < 3 || ch < 3) return null;
  const energy = new Float64Array(cw * ch);
  const lum = (x: number, y: number) => {
    const i = (y * W + x) * 4;
    return 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
  };
  for (let cy = 0; cy < ch; cy++) {
    for (let cx = 0; cx < cw; cx++) {
      let sum = 0;
      const x0 = cx * cell;
      const y0 = cy * cell;
      for (let y = y0 + 1; y < Math.min(y0 + cell, H - 1); y += 2) {
        for (let x = x0 + 1; x < Math.min(x0 + cell, W - 1); x += 2) {
          sum += Math.abs(lum(x + 1, y) - lum(x - 1, y)) + Math.abs(lum(x, y + 1) - lum(x, y - 1));
        }
      }
      energy[cy * cw + cx] = sum;
    }
  }

  // Somme d'aire (image intégrale) : fenêtre glissante en temps constant.
  const sat = new Float64Array((cw + 1) * (ch + 1));
  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      sat[(y + 1) * (cw + 1) + x + 1] =
        energy[y * cw + x] + sat[y * (cw + 1) + x + 1] + sat[(y + 1) * (cw + 1) + x] - sat[y * (cw + 1) + x];
    }
  }
  const winW = Math.max(1, Math.min(cw, Math.round(boxW / cell)));
  const winH = Math.max(1, Math.min(ch, Math.round(boxH / cell)));
  let best = -1;
  let bx = 0;
  let by = 0;
  for (let y = 0; y + winH <= ch; y++) {
    for (let x = 0; x + winW <= cw; x++) {
      const v =
        sat[(y + winH) * (cw + 1) + x + winW] - sat[y * (cw + 1) + x + winW] -
        sat[(y + winH) * (cw + 1) + x] + sat[y * (cw + 1) + x];
      if (v > best) { best = v; bx = x; by = y; }
    }
  }
  if (best <= 0) return null;
  return { x: ((bx + winW / 2) * cell) / W, y: ((by + winH / 2) * cell) / H };
}

function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

export function WatermarkStudio({ initialPaths, outputDir, onClose, onResults }: Props) {
  // ── Fichiers & source affichée ────────────────────────────────────────────
  const [files, setFiles] = useState<StudioFile[]>(() => initialPaths.map(studioFile));
  const [activeId, setActiveId] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [bg, setBg] = useState<CanvasImageSource & { width: number; height: number } | null>(null);
  const [page, setPage] = useState(1);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // ── Réglages + annuler/rétablir ───────────────────────────────────────────
  const [cfg, setCfg] = useState<WatermarkConfig>(loadLastConfig);
  const cfgRef = useRef(cfg);
  const committed = useRef(cfg);
  const past = useRef<WatermarkConfig[]>([]);
  const future = useRef<WatermarkConfig[]>([]);
  const [, setHistTick] = useState(0);

  const [logo, setLogo] = useState<HTMLImageElement | null>(null);
  const [fonts, setFonts] = useState<string[]>([]);
  const [fontTick, setFontTick] = useState(0);
  const [userPresets, setUserPresets] = useState<Preset[]>(loadUserPresets);
  const [notice, setNotice] = useState<string | null>(null);

  // ── Sortie ────────────────────────────────────────────────────────────────
  const [pageRange, setPageRange] = useState("");
  const [imageFormat, setImageFormat] = useState("same");
  const [quality, setQuality] = useState(92);
  const [audioFormat, setAudioFormat] = useState<"wav" | "flac">("flac");
  const [pdfFallback, setPdfFallback] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [results, setResults] = useState<Result[]>([]);

  // ── Aperçu ────────────────────────────────────────────────────────────────
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState<Box | null>(null);
  const [guides, setGuides] = useState({ v: false, h: false });
  const drag = useRef<Drag | null>(null);

  const active = files.find((f) => f.id === activeId) ?? null;

  // ── Réglages ──────────────────────────────────────────────────────────────

  const commit = useCallback(() => {
    const cur = cfgRef.current;
    if (JSON.stringify(cur) === JSON.stringify(committed.current)) return;
    past.current = [...past.current, committed.current].slice(-MAX_UNDO);
    future.current = [];
    committed.current = cur;
    saveLastConfig(cur);
    setHistTick((t) => t + 1);
  }, []);

  const change = useCallback((patch: Partial<WatermarkConfig>, doCommit = false) => {
    const next = { ...cfgRef.current, ...patch };
    cfgRef.current = next;
    setCfg(next);
    if (doCommit) commit();
  }, [commit]);

  const replace = useCallback((next: WatermarkConfig) => {
    cfgRef.current = next;
    committed.current = next;
    setCfg(next);
    saveLastConfig(next);
    setHistTick((t) => t + 1);
  }, []);

  const undo = useCallback(() => {
    const prev = past.current[past.current.length - 1];
    if (!prev) return;
    past.current = past.current.slice(0, -1);
    future.current = [cfgRef.current, ...future.current];
    replace(prev);
  }, [replace]);

  const redo = useCallback(() => {
    const next = future.current[0];
    if (!next) return;
    future.current = future.current.slice(1);
    past.current = [...past.current, cfgRef.current];
    replace(next);
  }, [replace]);

  // ── Chargements ───────────────────────────────────────────────────────────

  useEffect(() => {
    if (!activeId && files.length > 0) setActiveId(files[0].id);
  }, [files, activeId]);

  useEffect(() => {
    if (!active) { setLoaded(null); setBg(null); return; }
    let cancelled = false;
    let doc: Loaded | null = null;
    setLoading(true);
    setLoadError(null);
    loadSource(active)
      .then((l) => {
        doc = l;
        if (cancelled) { disposeLoaded(l); return; }
        setLoaded(l);
        setPage(1);
        const shown = l.kind === "archive" ? l.inner : l;
        if (shown?.kind === "image" || shown?.kind === "frame") setBg(shown.bg);
        else if (shown?.kind !== "pdf") setBg(null);
      })
      .catch((e) => { if (!cancelled) { setLoaded(null); setBg(null); setLoadError(errText(e)); } })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => {
      cancelled = true;
      disposeLoaded(doc);
    };
  }, [active?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  /** Un DOCX ou un PPTX ne sait pas fusionner une image avec son contenu :
   *  le mode de fusion n'y est pas appliqué, et l'aperçu ne doit pas le montrer. */
  const blendSupported = !active || ["image", "svg", "pdf", "pdfOnly"].includes(familyOf(active.ext)) || ["html", "htm"].includes(active.ext);

  /** Ce qui est affiché : le fichier, ou le premier fichier visuel d'une archive. */
  const shown = loaded?.kind === "archive" ? loaded.inner : loaded;
  const shownFamily: Family | null = shown?.kind === "frame" ? shown.family : active ? familyOf(active.ext) : null;

  useEffect(() => {
    if (shown?.kind !== "pdf") return;
    let cancelled = false;
    renderPdfPage(shown.doc, page)
      .then((c) => { if (!cancelled) setBg(c); })
      .catch((e) => { if (!cancelled) setLoadError(errText(e)); });
    return () => { cancelled = true; };
  }, [loaded, page]);

  useEffect(() => {
    if (!cfg.logoPath) { setLogo(null); return; }
    let cancelled = false;
    loadLogo(cfg.logoPath)
      .then((img) => { if (!cancelled) setLogo(img); })
      .catch((e) => { if (!cancelled) { setLogo(null); setNotice(`Logo illisible : ${errText(e)}`); } });
    return () => { cancelled = true; };
  }, [cfg.logoPath]);

  useEffect(() => {
    if (!cfg.fontFile) return;
    loadFontFile(cfg.fontFile)
      .then((family) => {
        if (cfgRef.current.fontFamily !== family) change({ fontFamily: family });
        setFonts((f) => (f.includes(family) ? f : [family, ...f]));
        setFontTick((t) => t + 1);
      })
      .catch((e) => setNotice(`Police illisible : ${errText(e)}`));
  }, [cfg.fontFile, change]);

  // Polices installées (Local Font Access) quand le webview l'autorise.
  useEffect(() => {
    const q = (window as unknown as { queryLocalFonts?: () => Promise<{ family: string }[]> }).queryLocalFonts;
    if (!q) return;
    q().then((list) => setFonts((f) => [...new Set([...f, ...list.map((x) => x.family)])].sort()))
      .catch(() => { /* refusé : la liste courante et la saisie libre restent disponibles */ });
  }, []);

  // Une police système se charge parfois après le premier rendu.
  useEffect(() => {
    const onLoad = () => setFontTick((t) => t + 1);
    document.fonts.addEventListener("loadingdone", onLoad);
    return () => document.fonts.removeEventListener("loadingdone", onLoad);
  }, []);

  // ── Rendu de l'aperçu ─────────────────────────────────────────────────────

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !bg) return;
    const id = requestAnimationFrame(() => {
      const W = bg.width;
      const H = bg.height;
      if (canvas.width !== W) canvas.width = W;
      if (canvas.height !== H) canvas.height = H;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, W, H);
      ctx.drawImage(bg, 0, 0, W, H);
      if (shown?.kind === "frame") {
        // Pas de calque graphique : le glisser déplace seulement la position.
        if (shown.family === "subtitle") drawSubtitlePreview(ctx, W, H, cfg);
        else drawTextDocPreview(ctx, W, H, cfg);
        setBox(null);
      } else {
        // Même chaîne qu'à l'export : calque séparé, puis fusion.
        const { canvas: layer, box: b } = renderLayer(W, H, cfg, logo);
        // « normal » s'appelle source-over dans le canvas ; les autres noms sont identiques.
        ctx.globalCompositeOperation =
          blendSupported && cfg.blend !== "normal" ? (cfg.blend as GlobalCompositeOperation) : "source-over";
        ctx.drawImage(layer, 0, 0, W, H);
        ctx.globalCompositeOperation = "source-over";
        setBox(b);
      }
    });
    return () => cancelAnimationFrame(id);
  }, [bg, cfg, logo, fontTick, shown]);

  // ── Souris : déplacer, redimensionner, pivoter ────────────────────────────

  function norm(e: { clientX: number; clientY: number }) {
    const r = wrapRef.current!.getBoundingClientRect();
    return { nx: (e.clientX - r.left) / r.width, ny: (e.clientY - r.top) / r.height, r };
  }

  /** `jump` : clic hors du filigrane, qui saute d'abord sous le curseur. */
  function startDrag(mode: DragMode, e: ReactPointerEvent, jump = false) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    wrapRef.current?.setPointerCapture(e.pointerId);
    const { nx, ny, r } = norm(e);
    if (jump) change({ x: nx, y: ny });
    const c = cfgRef.current;
    const d0 = Math.hypot((nx - c.x) * r.width, (ny - c.y) * r.height) || 1;
    drag.current = { mode, cfg0: c, nx0: nx, ny0: ny, d0 };
  }

  function onPointerMove(e: ReactPointerEvent) {
    const d = drag.current;
    if (!d) return;
    const { nx, ny, r } = norm(e);
    const c0 = d.cfg0;

    if (d.mode === "move") {
      const x = c0.x + (nx - d.nx0);
      const y = c0.y + (ny - d.ny0);
      // Aimant sur les axes centraux (Alt pour le désactiver).
      const v = !e.altKey && Math.abs(x - 0.5) < SNAP;
      const h = !e.altKey && Math.abs(y - 0.5) < SNAP;
      setGuides({ v, h });
      change({ x: v ? 0.5 : x, y: h ? 0.5 : y });
    } else if (d.mode === "scale") {
      const dist = Math.hypot((nx - c0.x) * r.width, (ny - c0.y) * r.height);
      change({ size: Math.min(200, Math.max(0.3, c0.size * (dist / d.d0))) });
    } else {
      let a = (Math.atan2((ny - c0.y) * r.height, (nx - c0.x) * r.width) * 180) / Math.PI + 90;
      if (a > 180) a -= 360;
      if (e.shiftKey) a = Math.round(a / 15) * 15;
      change({ rotation: Math.round(a * 10) / 10 });
    }
  }

  function endDrag() {
    if (!drag.current) return;
    drag.current = null;
    setGuides({ v: false, h: false });
    commit();
  }

  function onWheel(e: React.WheelEvent) {
    const c = cfgRef.current;
    if (e.shiftKey) change({ rotation: Math.max(-180, Math.min(180, c.rotation + (e.deltaY > 0 ? 1 : -1))) });
    else change({ size: Math.min(200, Math.max(0.3, c.size * (e.deltaY > 0 ? 0.95 : 1.05))) });
    commit();
  }

  // Clavier : annuler/rétablir, flèches pour ajuster au pixel près.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const t = e.target as HTMLElement;
      const typing = t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT";
      if (e.key === "Escape" && !progress) { onClose(); return; }
      if (typing) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); if (e.shiftKey) redo(); else undo(); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); return; }
      const step = e.shiftKey ? 0.02 : 0.002;
      const moves: Record<string, [number, number]> = {
        ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step],
      };
      const m = moves[e.key];
      if (m) {
        e.preventDefault();
        change({ x: cfgRef.current.x + m[0], y: cfgRef.current.y + m[1] }, true);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo, change, onClose, progress]);

  // ── Actions du panneau ────────────────────────────────────────────────────

  function anchor(ax: number, ay: number) {
    if (!bg) return;
    const margin = 0.03;
    // Demi-encombrement du bloc tourné, en fraction de la page (sous-titres : marge fixe).
    const c = box ? Math.abs(Math.cos(box.angle)) : 0;
    const s = box ? Math.abs(Math.sin(box.angle)) : 0;
    const hw = box ? (box.w * c + box.h * s) / 2 / bg.width : 0.12;
    const hh = box ? (box.w * s + box.h * c) / 2 / bg.height : 0.05;
    const place = (a: number, half: number) => (a === 0.5 ? 0.5 : a === 0 ? margin + half : 1 - margin - half);
    change({ x: place(ax, hw), y: place(ay, hh) }, true);
  }

/** Place le filigrane sur la zone la plus détaillée de l'aperçu. */
  function placeOnBusiest() {
    const canvas = canvasRef.current;
    if (!canvas || !bg || !box) { setNotice("Chargez d'abord un fichier avec une image"); return; }
    // On mesure le FOND seul, sans le filigrane déjà dessiné par-dessus.
    const clean = document.createElement("canvas");
    clean.width = bg.width;
    clean.height = bg.height;
    clean.getContext("2d")?.drawImage(bg, 0, 0, bg.width, bg.height);
    const spot = busiestSpot(clean, box.w, box.h);
    if (!spot) { setNotice("Zone détaillée introuvable sur cet aperçu"); return; }
    change({ x: spot.x, y: spot.y }, true);
    setNotice("Filigrane placé sur la zone la plus détaillée");
  }

  /** Lit la signature invisible d'une image choisie par l'utilisateur. */
  async function readSignature() {
    const p = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif"] }] });
    if (typeof p !== "string") return;
    try {
      const found = await invoke<string | null>("watermark_read_signature", { inputPath: p });
      setNotice(found ? `Signature trouvée : ${found}` : "Aucune signature invisible dans ce fichier");
    } catch (e) {
      setNotice(`Lecture impossible : ${errText(e)}`);
    }
  }

  function savePreset(name: string) {
    const next = [...userPresets.filter((p) => p.name !== name), { name, config: cfgRef.current }];
    setUserPresets(next);
    setNotice(saveUserPresets(next) ? `Préréglage « ${name} » enregistré` : "Stockage plein : préréglage non enregistré");
  }

  function deletePreset(name: string) {
    const next = userPresets.filter((p) => p.name !== name);
    setUserPresets(next);
    saveUserPresets(next);
  }

  async function addFiles() {
    const sel = await open({ multiple: true });
    if (!sel) return;
    const paths = Array.isArray(sel) ? sel : [sel];
    setFiles((prev) => {
      const known = new Set(prev.map((f) => f.path));
      return [...prev, ...paths.filter((p) => !known.has(p)).map(studioFile)];
    });
  }

  function removeFile(id: string) {
    setFiles((prev) => prev.filter((f) => f.id !== id));
    if (id === activeId) setActiveId(null);
  }

  // ── Application ───────────────────────────────────────────────────────────

  async function applyAll() {
    if (files.length === 0 || progress) return;
    commit();
    setResults([]);
    setProgress({ done: 0, total: files.length });
    const opts: ApplyOptions = {
      cfg: cfgRef.current, logo, imageFormat, audioFormat, quality, pageRange, pdfFallback, outputDir,
    };
    const out: Result[] = [];
    const history: HistoryItem[] = [];

    for (const [i, f] of files.entries()) {
      try {
        const res = await watermarkFile(f, opts, f.id === activeId ? loaded : null);
        out.push({ name: f.name, path: res.path, size: res.outputSize });
        history.push({
          id: `${Date.now()}-${Math.random()}`,
          inputName: f.name, inputExt: f.ext,
          outputPath: res.path, outputFormat: res.path.split(".").pop() ?? "",
          outputSize: res.outputSize, timestamp: Date.now(),
        });
      } catch (e) {
        out.push({ name: f.name, error: errText(e) });
      }
      setResults([...out]);
      setProgress({ done: i + 1, total: files.length });
    }

    setProgress(null);
    if (history.length) onResults(history);
  }

  // ── Rendu ─────────────────────────────────────────────────────────────────

  const families = new Set(files.map((f) => familyOf(f.ext)));
  const hasImage = families.has("image") || families.has("archive");
  const hasPdf = families.has("pdf") || (families.has("pdfOnly") && pdfFallback);
  const hasAudio = families.has("audio") || families.has("archive");
  const hasPdfOnly = families.has("pdfOnly") || families.has("archive");

  async function pickSound() {
    const p = await open({ multiple: false, filters: [{ name: "Audio", extensions: ["mp3", "wav", "flac", "ogg", "m4a", "aac"] }] });
    if (typeof p === "string") change({ audioSound: p }, true);
  }
  const pct = (v: number, of: number) => `${(v / of) * 100}%`;

  return (
    <div className="fixed inset-0 z-40 bg-slate-950 text-white flex flex-col">
      {/* En-tête */}
      <header className="h-11 shrink-0 border-b border-slate-800 flex items-center gap-2 px-3">
        <h2 className="text-sm font-bold">Studio filigrane</h2>
        <span className="text-[11px] text-slate-500 hidden md:inline">
          Glisser = déplacer · coin = taille · poignée ronde = rotation (Maj = pas de 15°) · molette = taille · Alt = sans aimant
        </span>
        <div className="ml-auto flex items-center gap-1">
          <button className={btn} onClick={undo} disabled={past.current.length === 0} title="Annuler (Ctrl+Z)">↶</button>
          <button className={btn} onClick={redo} disabled={future.current.length === 0} title="Rétablir (Ctrl+Y)">↷</button>
          <button className={btn} onClick={() => replace(DEFAULT_CONFIG)} title="Réglages par défaut">Réinitialiser</button>
          <button onClick={onClose} disabled={!!progress}
            className="ml-2 text-slate-400 hover:text-white px-2 text-lg leading-none" title="Fermer (Échap)">✕</button>
        </div>
      </header>

      <div className="flex-1 flex min-h-0">
        {/* Fichiers */}
        <aside className="w-44 shrink-0 border-r border-slate-800 flex flex-col">
          <button onClick={addFiles} className="m-2 bg-blue-600 hover:bg-blue-500 rounded-md text-xs py-1.5">+ Ajouter des fichiers</button>
          <div className="flex-1 overflow-y-auto">
            {files.map((f) => (
              <div key={f.id} onClick={() => setActiveId(f.id)}
                className={`group flex items-center gap-1 px-2 py-1.5 text-xs cursor-pointer border-l-2 ${
                  f.id === activeId ? "bg-slate-800 border-blue-500" : "border-transparent hover:bg-slate-900"}`}>
                <span className="text-[9px] uppercase text-slate-500 w-8 shrink-0">{f.ext}</span>
                <span className="truncate flex-1" title={f.path}>{f.name}</span>
                <button onClick={(e) => { e.stopPropagation(); removeFile(f.id); }}
                  className="opacity-0 group-hover:opacity-100 text-slate-500 hover:text-red-400">✕</button>
              </div>
            ))}
            {files.length === 0 && <p className="text-[11px] text-slate-600 p-3">Tous les formats d'UniversalConverter : images, PDF, Office, OpenDocument, EPUB, HTML, texte, sous-titres, audio, archives…</p>}
          </div>
          {shown?.kind === "pdf" && (
            <div className="border-t border-slate-800 p-2 flex items-center justify-between text-xs">
              <button className={btn} disabled={page <= 1} onClick={() => setPage(page - 1)}>‹</button>
              <span className="text-slate-400">Page {page}/{shown.sizes.length}</span>
              <button className={btn} disabled={page >= shown.sizes.length} onClick={() => setPage(page + 1)}>›</button>
            </div>
          )}
        </aside>

        {/* Aperçu */}
        <main className="relative flex-1 min-w-0 flex items-center justify-center p-4 pt-10 overflow-hidden"
          style={{ background: "repeating-conic-gradient(#1e293b 0% 25%, #0f172a 0% 50%) 50% / 20px 20px" }}>
          {active && shownFamily && !loading && (
            <p data-wm-info className="absolute top-2 inset-x-4 text-[11px] text-slate-300 bg-slate-900/90 border border-slate-700 rounded px-2 py-1 truncate"
              title={FAMILY_INFO[shownFamily]}>
              {loaded?.kind === "archive" && `${loaded.entries.length} fichier(s) dans l'archive${loaded.innerName ? `, aperçu : ${loaded.innerName}` : ""} · `}
              {FAMILY_INFO[loaded?.kind === "archive" ? "archive" : shownFamily]}
            </p>
          )}
          {loading && <p className="text-slate-400 text-sm">Chargement…</p>}
          {!loading && loadError && <p className="text-red-400 text-sm max-w-md text-center">{loadError}</p>}
          {!loading && !loadError && !bg && !active && <p className="text-slate-500 text-sm">Ajoutez un fichier pour commencer</p>}
          {!loading && !loadError && shown?.kind === "audio" && (
            <div data-wm-audio className="w-full max-w-md space-y-3 bg-slate-900 border border-slate-700 rounded-xl p-4">
              <p className="text-sm font-semibold">Tatouage sonore</p>
              <button className={`${btn} w-full`} onClick={pickSound}>Choisir le son à mixer…</button>
              <p className="text-[10px] text-slate-500 break-all">{cfg.audioSound ?? "Aucun son choisi (jingle, voix « aperçu », bip…)"}</p>
              <Slider label="Intervalle (0 = une seule fois)" value={cfg.audioInterval} min={0} max={300} step={1} unit="s"
                onChange={(v) => change({ audioInterval: v })} onCommit={commit} />
              <Slider label="Premier passage" value={cfg.audioOffset} min={0} max={300} step={0.5} unit="s"
                onChange={(v) => change({ audioOffset: v })} onCommit={commit} />
              <Slider label="Volume du son" value={Math.round(cfg.audioVolume * 100)} min={0} max={200} unit="%"
                onChange={(v) => change({ audioVolume: v / 100 })} onCommit={commit} />
              <p className="text-[10px] text-slate-500">Sortie WAV ou FLAC (sans perte) : aucun encodeur MP3/OGG/AAC n'est disponible dans le moteur.</p>
            </div>
          )}
          <div ref={wrapRef}
            className={`relative select-none touch-none cursor-crosshair ${bg && !loading && !loadError && shown?.kind !== "audio" ? "" : "hidden"}`}
            onPointerDown={(e) => startDrag("move", e, true)} onPointerMove={onPointerMove}
            onPointerUp={endDrag} onPointerCancel={endDrag} onWheel={onWheel}>
            <canvas data-wm-canvas ref={canvasRef} className="block max-w-full shadow-2xl"
              style={{ maxHeight: "calc(100vh - 44px - 64px - 32px)" }} />
            {guides.v && <div className="absolute inset-y-0 left-1/2 w-px bg-pink-500 pointer-events-none" />}
            {guides.h && <div className="absolute inset-x-0 top-1/2 h-px bg-pink-500 pointer-events-none" />}
            {box && bg && box.w > 0 && (
              <div data-wm-box onPointerDown={(e) => startDrag("move", e)}
                className="absolute border border-dashed border-sky-400 cursor-move"
                style={{
                  left: pct(box.cx - box.w / 2, bg.width), top: pct(box.cy - box.h / 2, bg.height),
                  width: pct(box.w, bg.width), height: pct(box.h, bg.height),
                  transform: `rotate(${box.angle}rad)`,
                }}>
                <div data-wm-handle="rotate" onPointerDown={(e) => startDrag("rotate", e)} title="Pivoter"
                  className="absolute left-1/2 -top-7 -translate-x-1/2 w-3.5 h-3.5 rounded-full bg-white border-2 border-sky-500 cursor-grab" />
                <div className="absolute left-1/2 -top-4 h-4 w-px bg-sky-400 pointer-events-none" />
                {["-left-1.5 -top-1.5", "-right-1.5 -top-1.5", "-left-1.5 -bottom-1.5", "-right-1.5 -bottom-1.5"].map((p) => (
                  <div key={p} data-wm-handle="scale" onPointerDown={(e) => startDrag("scale", e)} title="Redimensionner"
                    className={`absolute ${p} w-3 h-3 bg-white border-2 border-sky-500 cursor-nwse-resize`} />
                ))}
              </div>
            )}
          </div>
        </main>

        {/* Réglages */}
        <aside className="w-72 shrink-0 border-l border-slate-800 overflow-y-auto">
          <WatermarkPanel cfg={cfg} fonts={fonts} userPresets={userPresets}
            onChange={change} onCommit={commit} onAnchor={anchor}
            onImportFont={(path) => change({ fontFile: path }, true)}
            onSavePreset={savePreset} onDeletePreset={deletePreset}
            onPlaceOnBusiest={placeOnBusiest} onReadSignature={readSignature} blendSupported={blendSupported}
            onLoadPreset={(p) => { change(p.config, true); }} />
        </aside>
      </div>

      {/* Sortie */}
      <footer className="h-16 shrink-0 border-t border-slate-800 flex items-center gap-3 px-3 text-xs">
        {hasImage && (
          <label className="flex items-center gap-1 text-slate-400">
            Images en
            <select value={imageFormat} onChange={(e) => setImageFormat(e.target.value)}
              className="bg-slate-800 border border-slate-700 rounded px-1 py-1 text-slate-200">
              <option value="same">Format d'origine</option>
              {OUTPUT_IMAGE_FORMATS.map((f) => <option key={f} value={f}>{FORMAT_LABELS[f] ?? f.toUpperCase()}</option>)}
            </select>
          </label>
        )}
        {hasImage && (imageFormat === "jpg" || (imageFormat === "same" && files.some((f) => ["jpg", "jpeg"].includes(f.ext)))) && (
          <label className="flex items-center gap-1 text-slate-400">
            Qualité JPEG
            <input type="number" min={1} max={100} value={quality}
              onChange={(e) => setQuality(Math.min(100, Math.max(1, parseInt(e.target.value) || 90)))}
              className="w-12 bg-slate-800 border border-slate-700 rounded px-1 py-1 text-slate-200" />
          </label>
        )}
        {hasAudio && (
          <label className="flex items-center gap-1 text-slate-400">
            Audio en
            <select value={audioFormat} onChange={(e) => setAudioFormat(e.target.value as "wav" | "flac")}
              className="bg-slate-800 border border-slate-700 rounded px-1 py-1 text-slate-200">
              <option value="flac">FLAC</option>
              <option value="wav">WAV</option>
            </select>
          </label>
        )}
        {hasPdfOnly && (
          <label className="flex items-center gap-1 text-slate-400 cursor-pointer"
            title="CSV, JSON, DOC, PPT et XLS ne peuvent porter aucun filigrane sans altérer leurs données">
            <input type="checkbox" checked={pdfFallback} onChange={(e) => setPdfFallback(e.target.checked)} className="accent-blue-500" />
            CSV/JSON/DOC/PPT/XLS → PDF
          </label>
        )}
        {hasPdf && (
          <label className="flex items-center gap-1 text-slate-400">
            Pages
            <input value={pageRange} onChange={(e) => setPageRange(e.target.value)} placeholder="toutes (ex. 1-3,5)"
              className="w-32 bg-slate-800 border border-slate-700 rounded px-1.5 py-1 text-slate-200" />
          </label>
        )}
        <span className="text-slate-500 whitespace-nowrap" title={outputDir ?? "Dossier de sortie : réglable dans l'en-tête principal"}>
          → {outputDir ? outputDir.split(/[\\/]/).pop() : "dossier source"} · suffixe _filigrane
        </span>

        <div className="flex-1 min-w-0 flex items-center gap-2 overflow-x-auto">
          {notice && (
            <span className="text-amber-300 truncate" onClick={() => setNotice(null)} title="Cliquer pour masquer">{notice}</span>
          )}
          {results.map((r, i) => r.error ? (
            <span key={i} className="text-red-400 truncate" title={r.error}>✗ {r.name}</span>
          ) : (
            <button key={i} onClick={() => revealItemInDir(r.path!)} title={r.path}
              className="text-green-400 hover:underline whitespace-nowrap">✓ {r.name} ({formatBytes(r.size ?? 0)})</button>
          ))}
        </div>

        <button onClick={applyAll} disabled={files.length === 0 || !!progress}
          className="shrink-0 bg-blue-600 hover:bg-blue-500 disabled:opacity-40 rounded-lg px-4 py-2 font-medium">
          {progress ? `Application ${progress.done}/${progress.total}…` : `Appliquer à ${files.length} fichier${files.length > 1 ? "s" : ""}`}
        </button>
      </footer>
    </div>
  );
}
