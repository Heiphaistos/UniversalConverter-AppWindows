import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { FileItem } from "../types";

interface Props {
  fileIds: string[];
  allFiles: FileItem[];
  outputDir: string | null;
  onClose: () => void;
}

export function MergePromptModal({ fileIds, allFiles, outputDir, onClose }: Props) {
  const pdfFiles = allFiles.filter(
    (f) => fileIds.includes(f.id) && f.status === "done" && f.outputPath
  );

  const [mode, setMode] = useState<"pages" | "single">("pages");
  const [outputName, setOutputName] = useState("merged");
  const [status, setStatus] = useState<"idle" | "merging" | "done" | "error">("idle");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function merge() {
    if (pdfFiles.length < 2) return;
    setStatus("merging");
    setError(null);
    try {
      const name = outputName.trim() || "merged";
      const dir = outputDir ?? pdfFiles[0].outputPath!.split(/[\\/]/).slice(0, -1).join("/");
      const outPath = `${dir}/${name}.pdf`;
      const res = await invoke<string>("merge_pdfs_mode_command", {
        inputPaths: pdfFiles.map((f) => f.outputPath!),
        outputPath: outPath,
        mode,
      });
      setResult(res);
      setStatus("done");
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : typeof e === "string" ? e : String(e));
      setStatus("error");
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60" onClick={onClose} />
      <div className="relative bg-slate-900 border border-slate-700 rounded-2xl w-[500px] max-h-[80vh] flex flex-col shadow-2xl">

        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-slate-800">
          <div>
            <h2 className="font-bold text-slate-200">Fusionner les PDFs ?</h2>
            <p className="text-xs text-slate-500 mt-0.5">
              {pdfFiles.length} fichiers viennent d'être convertis en PDF
            </p>
          </div>
          <button onClick={onClose} className="text-slate-400 hover:text-white transition-colors">
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Corps */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">

          {/* Liste des PDFs */}
          <div className="space-y-1">
            {pdfFiles.map((f, i) => (
              <div key={f.id} className="flex items-center gap-2 bg-slate-800/60 rounded-lg px-3 py-1.5">
                <span className="text-xs text-slate-500 w-5">{i + 1}.</span>
                <span className="text-sm text-slate-300 truncate flex-1" title={f.outputPath}>
                  {f.outputPath?.split(/[\\/]/).pop()}
                </span>
              </div>
            ))}
          </div>

          {/* Mode de fusion */}
          <div className="space-y-2">
            <p className="text-xs text-slate-400 font-medium uppercase tracking-wider">Mode de fusion</p>

            <button
              onClick={() => setMode("pages")}
              className={`w-full text-left px-4 py-3 rounded-xl border transition-colors ${
                mode === "pages"
                  ? "border-blue-500 bg-blue-600/10 text-slate-100"
                  : "border-slate-700 bg-slate-800/50 text-slate-400 hover:border-slate-600"
              }`}
            >
              <p className="font-medium text-sm">Pages séparées</p>
              <p className="text-xs mt-0.5 opacity-70">
                Chaque page est distincte — layout, images et mise en page conservés
              </p>
            </button>

            <button
              onClick={() => setMode("single")}
              className={`w-full text-left px-4 py-3 rounded-xl border transition-colors ${
                mode === "single"
                  ? "border-blue-500 bg-blue-600/10 text-slate-100"
                  : "border-slate-700 bg-slate-800/50 text-slate-400 hover:border-slate-600"
              }`}
            >
              <p className="font-medium text-sm">Page unique</p>
              <p className="text-xs mt-0.5 opacity-70">
                Tout le texte sur une seule page défilante (hauteur variable)
              </p>
            </button>
          </div>

          {/* Nom de sortie */}
          <div className="flex items-center gap-3">
            <label className="text-sm text-slate-400 shrink-0">Nom :</label>
            <input
              value={outputName}
              onChange={(e) => setOutputName(e.target.value)}
              className="flex-1 bg-slate-800 border border-slate-700 rounded-lg px-3 py-1.5 text-sm text-slate-200 focus:outline-none focus:border-blue-500"
              placeholder="merged"
            />
            <span className="text-slate-500 text-sm">.pdf</span>
          </div>

          {/* Résultats */}
          {status === "done" && result && (
            <div className="flex items-center gap-2 bg-green-900/20 border border-green-900/40 rounded-lg px-3 py-2">
              <span className="text-green-400 text-sm flex-1 truncate">✓ {result.split(/[\\/]/).pop()}</span>
              <button onClick={() => revealItemInDir(result)}
                className="text-xs text-slate-400 hover:text-slate-200 shrink-0">
                Afficher
              </button>
            </div>
          )}
          {status === "error" && error && (
            <p className="text-red-400 text-sm bg-red-900/20 border border-red-900/40 rounded-lg px-3 py-2">✗ {error}</p>
          )}
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t border-slate-800 flex justify-end gap-3">
          <button onClick={onClose}
            className="text-sm text-slate-400 hover:text-slate-200 px-4 py-2 rounded-lg transition-colors">
            Ignorer
          </button>
          {status !== "done" && (
            <button
              onClick={merge}
              disabled={pdfFiles.length < 2 || status === "merging"}
              className="bg-blue-600 hover:bg-blue-500 disabled:opacity-40 text-white text-sm px-5 py-2 rounded-lg font-medium transition-colors"
            >
              {status === "merging" ? "Fusion…" : "Fusionner"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
