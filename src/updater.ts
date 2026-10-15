// Mise à jour automatique : le plugin Tauri vérifie la signature minisign du
// manifeste latest.json (clé publique dans tauri.conf.json), télécharge
// l'installeur et le lance en mode passif ; relaunch() redémarre l'application.
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";

export const AUTO_CHECK_KEY = "uc_auto_update_check";

type Store = Pick<Storage, "getItem" | "setItem">;

/** Réglage « vérifier automatiquement » : actif par défaut, seul "0" le coupe. */
export function loadAutoCheck(store: Store = localStorage): boolean {
  try { return store.getItem(AUTO_CHECK_KEY) !== "0"; } catch { return true; }
}

export function saveAutoCheck(on: boolean, store: Store = localStorage): void {
  try { store.setItem(AUTO_CHECK_KEY, on ? "1" : "0"); } catch { /* stockage indisponible */ }
}

/** Pourcentage de téléchargement, ou null si la taille totale est inconnue. */
export function percent(done: number, total: number | undefined): number | null {
  return total && total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null;
}

/**
 * Cherche une mise à jour, la propose, l'installe puis redémarre.
 * silent = true : rien n'est affiché s'il n'y a rien ou si le canal est injoignable.
 * Retourne true si une installation a été lancée.
 */
export async function checkForUpdate(
  silent: boolean,
  onProgress?: (pct: number | null) => void,
): Promise<boolean> {
  if (import.meta.env.DEV) {
    if (!silent) await message("Vérification désactivée en mode développement.", { title: "Mise à jour" });
    return false;
  }
  try {
    const update = await check();
    if (!update) {
      if (!silent) await message("Universal Converter est à jour.", { title: "Mise à jour" });
      return false;
    }
    const ok = await ask(
      `La version ${update.version} est disponible (vous avez la ${update.currentVersion}).` +
        (update.body ? `\n\n${update.body}` : "") +
        "\n\nVoulez-vous l'installer maintenant ? L'application redémarrera.",
      { title: "Nouvelle version disponible", kind: "info", okLabel: "Installer", cancelLabel: "Plus tard" },
    );
    if (!ok) return false;

    let total: number | undefined;
    let done = 0;
    await update.downloadAndInstall((e) => {
      if (e.event === "Started") total = e.data.contentLength;
      else if (e.event === "Progress") { done += e.data.chunkLength; onProgress?.(percent(done, total)); }
    });
    await relaunch();
    return true;
  } catch (e) {
    console.error("[maj] échec", e);
    if (!silent) await message(`Vérification impossible : ${String(e)}`, { title: "Mise à jour", kind: "error" });
    return false;
  }
}
