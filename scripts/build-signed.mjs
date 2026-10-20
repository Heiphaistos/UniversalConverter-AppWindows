// Lance `tauri build` avec la clé de signature des mises à jour (hors dépôt).
// Clé et mot de passe (hors dépôt) : $UC_UPDATER_KEY, sinon $MDP_DIR/universalconverter-updater.key (+ .password). Les clés Tauri 2 « sans mot de passe »
// générées par le CLI 2.10/2.12 ne se rouvrent pas ; on utilise donc un mot de passe.
// Arguments passés à tauri build.
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

import { join } from "node:path";

function cle() {
  if (process.env.UC_UPDATER_KEY) return process.env.UC_UPDATER_KEY;
  if (process.env.MDP_DIR) return join(process.env.MDP_DIR, "universalconverter-updater.key");
  console.error("UC_UPDATER_KEY (ou MDP_DIR) non défini : chemin de la clé de signature des mises à jour");
  process.exit(1);
}
const env = {
  ...process.env,
  TAURI_SIGNING_PRIVATE_KEY: process.env.TAURI_SIGNING_PRIVATE_KEY ?? readFileSync(cle(), "utf-8").trim(),
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD:
    process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? readFileSync(cle().replace(/\.key$/, ".password"), "utf-8").trim(),
};
const r = spawnSync("npm", ["run", "tauri", "build", "--", ...process.argv.slice(2)], { env, stdio: "inherit", shell: true });
process.exit(r.status ?? 1);
