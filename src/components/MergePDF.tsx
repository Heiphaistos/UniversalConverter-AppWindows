import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

interface Props {
  outputDir: string | null;
  onClose: () => void;
}

export function MergePDF({ outputDir, onClose }: Props) {
  const [pdfs, setPdfs] = useState<string[]>([]);
  const [outputName, setOutputName] = useState("merged");
  const [mode, setMode] = useState<"pages" | "single">("pages");
  const [status, setStatus] = useState<"idle" | "merging" | "done" | "error">("idle");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function addPdfs() {
    const selected = await open({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setPdfs((prev) => {
      const existing = new Set(prev);
      return [...prev, ...paths.filter((p) => !existing.has(p))];
    });
  }

  function removePdf(path: string) {
    setPdfs((prev) => prev.filter((p) => p !== path));
  }

  function moveUp(idx: number) {
    if (idx === 0) return;
    setPdfs((prev) => {
      const arr = [...prev];
      [arr[idx - 1], arr[idx]] = [arr[idx], arr[idx - 1]];
      return arr;
    });
  }

  function moveDown(idx: number) {
    setPdfs((prev) => {
      if (idx >= prev.length - 1) return prev;
      const arr = [...prev];
      [arr[idx], arr[idx + 1]] = [arr[idx + 1], arr[idx]];
      return arr;
    });
  }

  async function merge() {
    if (pdfs.length < 2) return;
    setStatus("merging");
    setError(null);
    try {
      // Sanitiser le nom de sortie : interdire '/', '\\', '..', null bytes, et caractères Windows interdits
      const raw = outputName.trim() || "merged";
      const name = raw
        .replace(/\0/g, "")
        .replace(/[/\\:*?"<>|]/g, "_")
        .replace(/\.\./g, "")
        .replace(/^\.+|\.+$/g, "")
        .trim() || "merged";
      const dir = outputDir ?? pdfs[0].split(/[\\/]/).slice(0, -1).join("/");
      const outPath = `${dir}/${name}.pdf`;
      const res = await invoke<string>("merge_pdfs_mode_command", {
        inputPaths: pdfs,
        outputPath: outPath,
        mode,
      });
      setResult(res);
      setStatus("done");
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
      setStatus("error");
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/50" onClick={onClose} />
      <div className="relative bg-slate-900 border border-slate-700 rounded-2xl w-[540px] max-h-[80vh] flex flex-col shadow-2xl">

        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-slate-800">
          <h2 className="font-bold text-slate-200">Fusionner des PDFs</h2>
          <button onClick={onClose} className="text-slate-400 hover:text-white transition-colors">
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Corps */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Liste des PDFs */}
          <div className="space-y-2">
            {pdfs.length === 0 ? (
              <p className="text-slate-500 text-sm text-center py-4">
                Ajoutez au moins 2 fichiers PDF à fusionner
              </p>
            ) : (
              pdfs.map((p, i) => (
                <div key={p} className="flex items-center gap-2 bg-slate-800 rounded-lg px-3 py-2">
                  <span className="text-xs text-slate-500 w-5 text-center">{i + 1}</span>
                  <span className="text-sm text-slate-300 flex-1 truncate" title={p}>
                    {p.split(/[\\/]/).pop()}
                  </span>
                  <button onClick={() => moveUp(i)} disabled={i === 0}
                    className="text-slate-500 hover:text-slate-300 disabled:opacity-20 transition-colors p-0.5">
                    ▲
                  </button>
                  <button onClick={() => moveDown(i)} disabled={i === pdfs.length - 1}
                    className="text-slate-500 hover:text-slate-300 disabled:opacity-20 transition-colors p-0.5">
                    ▼
                  </button>
                  <button onClick={() => removePdf(p)}
                    className="text-slate-500 hover:text-red-400 transition-colors p-0.5">
                    ✕
                  </button>
                </div>
              ))
            )}
          </div>

          <button onClick={addPdfs}
            className="w-full border-2 border-dashed border-slate-600 hover:border-slate-400 rounded-lg py-2 text-sm text-slate-400 hover:text-slate-200 transition-colors">
            + Ajouter des PDFs
          </button>

          {/* Mode de fusion */}
          <div className="grid grid-cols-2 gap-2">
            <button
              onClick={() => setMode("pages")}
              className={`text-left px-3 py-2 rounded-lg border text-xs transition-colors ${
                mode === "pages"
                  ? "border-blue-500 bg-blue-600/10 text-slate-100"
                  : "border-slate-700 bg-slate-800 text-slate-400 hover:border-slate-600"
              }`}
            >
              <p className="font-medium">Pages séparées</p>
              <p className="opacity-70 mt-0.5">Layout + images conservés</p>
            </button>
            <button
              onClick={() => setMode("single")}
              className={`text-left px-3 py-2 rounded-lg border text-xs transition-colors ${
                mode === "single"
                  ? "border-blue-500 bg-blue-600/10 text-slate-100"
                  : "border-slate-700 bg-slate-800 text-slate-400 hover:border-slate-600"
              }`}
            >
              <p className="font-medium">Page unique</p>
              <p className="opacity-70 mt-0.5">Texte condensé sur une page</p>
            </button>
          </div>

          {/* Nom de sortie */}
          <div className="flex items-center gap-3">
            <label className="text-sm text-slate-400 shrink-0">Nom de sortie :</label>
            <input
              value={outputName}
              onChange={(e) => setOutputName(e.target.value)}
              className="flex-1 bg-slate-800 border border-slate-700 rounded-lg px-3 py-1.5 text-sm text-slate-200 focus:outline-none focus:border-blue-500"
              placeholder="merged"
            />
            <span className="text-slate-500 text-sm">.pdf</span>
          </div>

          {/* Résultat */}
          {status === "done" && result && (
            <div className="flex items-center gap-2 bg-green-900/20 border border-green-900/40 rounded-lg px-3 py-2">
              <span className="text-green-400 text-sm flex-1 truncate">✓ {result.split(/[\\/]/).pop()}</span>
              <button onClick={() => revealItemInDir(result)}
                className="text-xs text-slate-400 hover:text-slate-200 transition-colors">
                Afficher
              </button>
            </div>
          )}
          {status === "error" && error && (
            <p className="text-red-400 text-sm bg-red-900/20 border border-red-900/40 rounded-lg px-3 py-2">
              ✗ {error}
            </p>
          )}
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t border-slate-800 flex justify-end gap-3">
          <button onClick={onClose}
            className="text-sm text-slate-400 hover:text-slate-200 px-4 py-2 rounded-lg transition-colors">
            Annuler
          </button>
          <button
            onClick={merge}
            disabled={pdfs.length < 2 || status === "merging"}
            className="bg-blue-600 hover:bg-blue-500 disabled:opacity-40 text-white text-sm px-5 py-2 rounded-lg font-medium transition-colors"
          >
            {status === "merging" ? "Fusion en cours…" : "Fusionner"}
          </button>
        </div>
      </div>
    </div>
  );
}
