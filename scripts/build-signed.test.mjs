// Sans UC_UPDATER_KEY ni MDP_DIR, build-signed.mjs refuse avec un message clair. Lancer : node scripts/build-signed.test.mjs
import { spawnSync } from "node:child_process";
import assert from "node:assert";
const env = { ...process.env };
for (const k of ["UC_UPDATER_KEY", "MDP_DIR", "TAURI_SIGNING_PRIVATE_KEY"]) delete env[k];
const r = spawnSync("node", ["scripts/build-signed.mjs"], { env, encoding: "utf8" });
assert.notStrictEqual(r.status, 0);
assert.match(r.stderr, /UC_UPDATER_KEY/);
console.log("ok");
