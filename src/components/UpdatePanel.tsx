import { useState } from "react";
import { checkForUpdate, loadAutoCheck, saveAutoCheck } from "../updater";

export default function UpdatePanel({ version, onClose }: { version: string; onClose: () => void }) {
  const [auto, setAuto] = useState(loadAutoCheck);
  const [busy, setBusy] = useState(false);
  const [pct, setPct] = useState<number | null>(null);

  async function manual() {
    setBusy(true);
    setPct(null);
    await checkForUpdate(false, setPct);
    setBusy(false);
  }

  return (
    <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center" onClick={onClose}>
      <div className="bg-slate-900 border border-slate-700 rounded-xl p-5 w-80 space-y-4" onClick={(e) => e.stopPropagation()}>
        <h2 className="text-sm font-bold">À propos et mises à jour</h2>
        <p className="text-xs text-slate-400">Universal Converter v{version}</p>
        <label className="flex items-center gap-2 text-xs text-slate-300 cursor-pointer">
          <input type="checkbox" checked={auto}
            onChange={(e) => { setAuto(e.target.checked); saveAutoCheck(e.target.checked); }} />
          Vérifier les mises à jour automatiquement
        </label>
        <button onClick={manual} disabled={busy}
          className="w-full bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white text-xs px-3 py-2 rounded-lg font-medium transition-colors">
          {busy ? (pct === null ? "Recherche en cours…" : `Téléchargement ${pct} %`) : "Rechercher une mise à jour"}
        </button>
        <button onClick={onClose} className="w-full text-xs text-slate-400 hover:text-slate-200">Fermer</button>
      </div>
    </div>
  );
}
