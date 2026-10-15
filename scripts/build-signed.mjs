// Lance `tauri build` avec la clé de signature des mises à jour (hors dépôt).
// Clé et mot de passe : D:/MDP (hors dépôt). Les clés Tauri 2 « sans mot de passe »
// générées par le CLI 2.10/2.12 ne se rouvrent pas ; on utilise donc un mot de passe.
// Arguments passés à tauri build.
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const KEY = process.env.UC_UPDATER_KEY ?? "D:/MDP/universalconverter-updater.key";
const env = {
  ...process.env,
  TAURI_SIGNING_PRIVATE_KEY: process.env.TAURI_SIGNING_PRIVATE_KEY ?? readFileSync(KEY, "utf-8").trim(),
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD:
    process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? readFileSync(KEY.replace(/\.key$/, ".password"), "utf-8").trim(),
};
const r = spawnSync("npm", ["run", "tauri", "build", "--", ...process.argv.slice(2)], { env, stdio: "inherit", shell: true });
process.exit(r.status ?? 1);
