import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir, openPath } from "@tauri-apps/plugin-opener";
import {
  FileItem, FORMAT_LABELS, IMAGE_EXTENSIONS,
  Rotation, formatBytes, parsePageRange,
} from "../types";
import { saveDefaultFormat } from "./FileUploader";

interface Props {
  files: FileItem[];
  onUpdate: (id: string, patch: Partial<FileItem>) => void;
  onRemove: (id: string) => void;
  onConvert: (file: FileItem) => void;
}

export function FileList({ files, onUpdate, onRemove, onConvert }: Props) {
  if (files.length === 0) return null;
  return (
    <div className="mt-6 space-y-3">
      <h2 className="text-slate-400 font-semibold text-xs uppercase tracking-wider">
        Fichiers ({files.length})
      </h2>
      {files.map((file) => (
        <FileRow
          key={file.id}
          file={file}
          onUpdate={(patch) => onUpdate(file.id, patch)}
          onConvert={() => onConvert(file)}
          onRemove={() => onRemove(file.id)}
        />
      ))}
    </div>
  );
}

// ── Ligne individuelle ─────────────────────────────────────────────────────────

interface RowProps {
  file: FileItem;
  onUpdate: (patch: Partial<FileItem>) => void;
  onConvert: () => void;
  onRemove: () => void;
}

function FileRow({ file, onUpdate, onConvert, onRemove }: RowProps) {
  const [splitInput, setSplitInput] = useState("");
  const [splitStatus, setSplitStatus] = useState<"idle"|"busy"|"done"|"error">("idle");
  const [splitResult, setSplitResult] = useState<string | null>(null);

  const busy = file.status === "converting";
  const done = file.status === "done";
  const isImage = IMAGE_EXTENSIONS.has(file.extension);
  const showQuality = ["jpg","jpeg"].includes(file.selectedFormat);

  function handleFormatChange(fmt: string) {
    onUpdate({ selectedFormat: fmt });
    if (fmt !== file.extension) saveDefaultFormat(file.extension, fmt);
  }

  async function handleSplit() {
    if (!file.pageCount || !splitInput.trim()) return;
    let pages: number[];
    try {
      pages = parsePageRange(splitInput, file.pageCount);
    } catch (e: any) {
      setSplitResult(String(e.message ?? e));
      setSplitStatus("error");
      return;
    }
    setSplitStatus("busy");
    try {
      const dir = file.path.split(/[\\/]/).slice(0, -1).join("/");
      // Sanitiser le stem pour éviter path traversal dans le nom de sortie
      const rawStem = file.name.replace(/\.[^.]+$/, "");
      const stem = rawStem
        .replace(/\0/g, "")
        .replace(/[/\\:*?"<>|]/g, "_")
        .replace(/\.\./g, "")
        .replace(/^\.+|\.+$/g, "")
        .trim() || "output";
      const outPath = `${dir}/${stem}_pages.pdf`;
      const res = await invoke<string>("split_pdf_command", {
        inputPath: file.path,
        pages,
        outputPath: outPath,
      });
      setSplitResult(res);
      setSplitStatus("done");
    } catch (e: any) {
      setSplitResult(String(e));
      setSplitStatus("error");
    }
  }

  return (
    <div className={`rounded-xl transition-colors ${done ? "bg-slate-800/60 border border-green-900/40" : "bg-slate-800"}`}>
      {/* ── Ligne principale ── */}
      <div className="flex items-center gap-2 px-4 py-3">
        {/* Thumbnail ou badge extension */}
        {file.thumbnail ? (
          <img src={file.thumbnail} alt="" className="w-10 h-10 rounded object-cover shrink-0 bg-slate-700" />
        ) : (
          <span className="text-xs font-bold bg-slate-700 text-slate-300 rounded px-1.5 py-1 uppercase w-10 text-center shrink-0">
            {file.extension || "?"}
          </span>
        )}

        {/* Infos */}
        <div className="flex-1 min-w-0">
          <p className="text-slate-200 text-sm font-medium truncate" title={file.path}>{file.name}</p>
          <div className="flex items-center gap-2 mt-0.5">
            {file.fileSize !== undefined && (
              <span className="text-xs text-slate-500">{formatBytes(file.fileSize)}</span>
            )}
            {file.pageCount !== undefined && (
              <span className="text-xs text-slate-500">{file.pageCount} page(s)</span>
            )}
          </div>
          {file.status === "error" && (
            <p className="text-red-400 text-xs mt-0.5 truncate" title={file.errorMessage}>✗ {file.errorMessage}</p>
          )}
          {busy && (
            <div className="mt-1.5 h-1.5 w-full bg-slate-700 rounded-full overflow-hidden">
              <div className="h-full bg-blue-500 rounded-full transition-all duration-200" style={{ width: `${file.progress}%` }} />
            </div>
          )}
        </div>

        {/* Sélecteur format */}
        {!done && (
          <select
            disabled={busy || file.availableFormats.length === 0}
            value={file.selectedFormat}
            onChange={(e) => handleFormatChange(e.target.value)}
            className="bg-slate-700 text-slate-200 text-sm rounded-lg px-2 py-1.5 border border-slate-600 focus:outline-none focus:border-blue-500 disabled:opacity-50 shrink-0"
          >
            {file.availableFormats.map((f) => (
              <option key={f} value={f}>→ {FORMAT_LABELS[f] ?? f.toUpperCase()}</option>
            ))}
          </select>
        )}

        {/* Bouton options (images) */}
        {!done && isImage && (
          <button
            onClick={() => onUpdate({ showOptions: !file.showOptions })}
            className={`p-1.5 rounded-lg transition-colors shrink-0 ${file.showOptions ? "bg-slate-600 text-white" : "text-slate-400 hover:text-slate-200 hover:bg-slate-700"}`}
            title="Options de conversion"
          >
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                d="M12 6V4m0 2a2 2 0 100 4m0-4a2 2 0 110 4m-6 8a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4m6 6v10m6-2a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4" />
            </svg>
          </button>
        )}

        {/* Convertir */}
        {!done && (
          <button
            onClick={onConvert}
            disabled={busy || !file.selectedFormat}
            className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-colors shrink-0
              ${busy ? "bg-slate-700 text-slate-400 cursor-not-allowed" : "bg-blue-600 hover:bg-blue-500 text-white disabled:opacity-40"}`}
          >
            {busy ? "…" : "Convertir"}
          </button>
        )}

        {/* Supprimer */}
        <button onClick={onRemove} disabled={busy}
          className="text-slate-500 hover:text-red-400 transition-colors disabled:opacity-30 shrink-0" title="Retirer">
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* ── Options image (collapsible) ── */}
      {file.showOptions && !done && isImage && (
        <div className="px-4 pb-3 pt-0 space-y-3 border-t border-slate-700/60">
          {/* Nom de sortie */}
          <div className="flex items-center gap-2 pt-3">
            <label className="text-xs text-slate-400 w-24 shrink-0">Nom fichier :</label>
            <input
              value={file.outputName}
              onChange={(e) => onUpdate({ outputName: e.target.value })}
              placeholder={`${file.name.replace(/\.[^.]+$/, "")}_converted`}
              className="flex-1 bg-slate-700 border border-slate-600 rounded-lg px-2 py-1 text-xs text-slate-200 focus:outline-none focus:border-blue-500"
            />
          </div>

          {/* Redimensionnement */}
          <div className="flex items-center gap-2">
            <label className="text-xs text-slate-400 w-24 shrink-0">Taille (px) :</label>
            <input
              type="number" min="1" placeholder="Largeur"
              value={file.imageOptions.resizeWidth}
              onChange={(e) => onUpdate({ imageOptions: { ...file.imageOptions, resizeWidth: e.target.value } })}
              className="w-24 bg-slate-700 border border-slate-600 rounded-lg px-2 py-1 text-xs text-slate-200 focus:outline-none focus:border-blue-500"
            />
            <span className="text-slate-500 text-xs">×</span>
            <input
              type="number" min="1" placeholder="Hauteur"
              value={file.imageOptions.resizeHeight}
              onChange={(e) => onUpdate({ imageOptions: { ...file.imageOptions, resizeHeight: e.target.value } })}
              className="w-24 bg-slate-700 border border-slate-600 rounded-lg px-2 py-1 text-xs text-slate-200 focus:outline-none focus:border-blue-500"
            />
            <span className="text-xs text-slate-500">(vide = ratio conservé)</span>
          </div>

          {/* Rotation */}
          <div className="flex items-center gap-2">
            <label className="text-xs text-slate-400 w-24 shrink-0">Rotation :</label>
            {([0, 90, 180, 270] as Rotation[]).map((deg) => (
              <button
                key={deg}
                onClick={() => onUpdate({ imageOptions: { ...file.imageOptions, rotation: deg } })}
                className={`px-2.5 py-1 rounded text-xs font-medium transition-colors
                  ${file.imageOptions.rotation === deg ? "bg-blue-600 text-white" : "bg-slate-700 text-slate-400 hover:bg-slate-600"}`}
              >
                {deg}°
              </button>
            ))}
          </div>

          {/* Qualité JPEG */}
          {showQuality && (
            <div className="flex items-center gap-3">
              <label className="text-xs text-slate-400 w-24 shrink-0">Qualité JPEG :</label>
              <input
                type="range" min="1" max="100"
                value={file.imageOptions.quality}
                onChange={(e) => onUpdate({ imageOptions: { ...file.imageOptions, quality: Number(e.target.value) } })}
                className="flex-1 accent-blue-500"
              />
              <span className="text-xs text-slate-300 w-8 text-right">{file.imageOptions.quality}%</span>
            </div>
          )}
        </div>
      )}

      {/* ── Split PDF ── */}
      {file.extension === "pdf" && file.pageCount && file.pageCount > 1 && !done && (
        <div className="px-4 pb-3 border-t border-slate-700/60">
          <div className="flex items-center gap-2 pt-2">
            <label className="text-xs text-slate-400 shrink-0">Extraire pages :</label>
            <input
              value={splitInput}
              onChange={(e) => setSplitInput(e.target.value)}
              placeholder={`ex: 1,3,5-7 (sur ${file.pageCount})`}
              className="flex-1 bg-slate-700 border border-slate-600 rounded-lg px-2 py-1 text-xs text-slate-200 focus:outline-none focus:border-blue-500"
            />
            <button
              onClick={handleSplit}
              disabled={splitStatus === "busy" || !splitInput.trim()}
              className="text-xs bg-slate-700 hover:bg-slate-600 text-slate-200 px-3 py-1 rounded-lg disabled:opacity-40 transition-colors shrink-0"
            >
              {splitStatus === "busy" ? "…" : "Extraire"}
            </button>
          </div>
          {splitStatus === "done" && splitResult && (
            <p className="text-green-400 text-xs mt-1 truncate">✓ {splitResult.split(/[\\/]/).pop()}</p>
          )}
          {splitStatus === "error" && splitResult && (
            <p className="text-red-400 text-xs mt-1">{splitResult}</p>
          )}
        </div>
      )}

      {/* ── Résultat post-conversion ── */}
      {done && file.outputPath && (
        <div className="px-4 pb-3 flex items-center gap-2 border-t border-green-900/20 pt-2">
          <div className="flex-1 min-w-0">
            <p className="text-green-400 text-xs truncate" title={file.outputPath}>
              ✓ {file.outputPath.split(/[\\/]/).pop()}
            </p>
            {file.outputSize !== undefined && (
              <p className="text-xs text-slate-500">
                {file.fileSize !== undefined && (
                  <>
                    {formatBytes(file.fileSize)} → {formatBytes(file.outputSize)}
                    {" "}
                    <span className={file.outputSize < file.fileSize ? "text-green-500" : "text-slate-400"}>
                      ({file.outputSize < file.fileSize
                        ? `-${Math.round((1 - file.outputSize / file.fileSize) * 100)}%`
                        : `+${Math.round((file.outputSize / file.fileSize - 1) * 100)}%`})
                    </span>
                  </>
                )}
              </p>
            )}
          </div>
          <button onClick={() => openPath(file.outputPath!)}
            className="flex items-center gap-1 text-xs text-blue-400 hover:text-blue-300 bg-blue-950/40 hover:bg-blue-950/70 px-2.5 py-1 rounded-lg transition-colors shrink-0">
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
            </svg>
            Ouvrir
          </button>
          <button onClick={() => revealItemInDir(file.outputPath!)}
            className="flex items-center gap-1 text-xs text-slate-400 hover:text-slate-200 bg-slate-700 hover:bg-slate-600 px-2.5 py-1 rounded-lg transition-colors shrink-0">
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V7z" />
            </svg>
            Explorateur
          </button>
        </div>
      )}
    </div>
  );
}
