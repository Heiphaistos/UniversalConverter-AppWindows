// Assemble latest.json (manifeste de mise à jour Tauri) à côté de l'installeur NSIS signé.
// Usage : node scripts/make-latest-json.mjs   (après `npm run tauri build`)
// À joindre à la release GitHub vX.Y.Z : installeur .exe + .exe.sig + latest.json.
import { readFileSync, writeFileSync, readdirSync } from "node:fs";

const REPO = "Heiphaistos/UniversalConverter-AppWindows";
const { version } = JSON.parse(readFileSync("package.json", "utf-8"));
const dir = "src-tauri/target/release/bundle/nsis";
const exe = readdirSync(dir).find((f) => f.includes(`_${version}_`) && f.endsWith("-setup.exe"));
if (!exe) throw new Error(`Installeur ${version} introuvable dans ${dir}`);

const signature = readFileSync(`${dir}/${exe}.sig`, "utf-8").trim();
const notes = (readFileSync("RELEASE_NOTES.md", "utf-8").replace(//g, "").split(/^## /m)
  .find((s) => s.startsWith(`v${version} `)) ?? "").split("\n").slice(1).join("\n").trim();

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url: `https://github.com/${REPO}/releases/download/v${version}/${encodeURIComponent(exe)}`,
    },
  },
};
writeFileSync(`${dir}/latest.json`, JSON.stringify(manifest, null, 2));
console.log(`OK ${dir}/latest.json (v${version}) -> joindre ${exe}, ${exe}.sig, latest.json à la release v${version}`);
