// Panneau de réglages du filigrane (colonne de droite du studio).

import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  WatermarkConfig, FillMode, TextAlign, BlendMode, Preset, BUILTIN_PRESETS, COMMON_FONTS, rgba,
} from "./config";
import { Section, Slider, ColorAlpha, Toggle, Segmented, btn } from "./controls";

const BLEND_MODES: { value: BlendMode; label: string }[] = [
  { value: "normal", label: "Normal (transparence)" },
  { value: "multiply", label: "Produit" },
  { value: "screen", label: "Superposition claire" },
  { value: "overlay", label: "Incrustation" },
  { value: "hard-light", label: "Lumière crue" },
  { value: "soft-light", label: "Lumière tamisée" },
  { value: "color-burn", label: "Densité couleur +" },
  { value: "color-dodge", label: "Densité couleur −" },
  { value: "darken", label: "Obscurcir" },
  { value: "lighten", label: "Éclaircir" },
  { value: "difference", label: "Différence (inversion)" },
  { value: "exclusion", label: "Exclusion" },
];

interface Props {
  cfg: WatermarkConfig;
  fonts: string[];
  userPresets: Preset[];
  onChange: (patch: Partial<WatermarkConfig>, commit?: boolean) => void;
  onCommit: () => void;
  onAnchor: (ax: number, ay: number) => void;
  onImportFont: (path: string) => void;
  onPlaceOnBusiest: () => void;
  onReadSignature: () => void;
  blendSupported: boolean;
  onSavePreset: (name: string) => void;
  onDeletePreset: (name: string) => void;
  onLoadPreset: (p: Preset) => void;
}

const IMAGE_FILTER = [{
  name: "Images",
  extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tiff", "tif", "svg", "ico", "tga", "qoi"],
}];

export function WatermarkPanel({
  cfg, fonts, userPresets, onChange, onCommit, onAnchor, onImportFont,
  onPlaceOnBusiest, onReadSignature, blendSupported,
  onSavePreset, onDeletePreset, onLoadPreset,
}: Props) {
  const [presetName, setPresetName] = useState("");
  const set = onChange;
  const allPresets = [...BUILTIN_PRESETS, ...userPresets];

  async function pickLogo() {
    const p = await open({ multiple: false, filters: IMAGE_FILTER });
    if (typeof p === "string") set({ kind: "image", logoPath: p }, true);
  }

  async function pickFont() {
    const p = await open({ multiple: false, filters: [{ name: "Polices", extensions: ["ttf", "otf", "woff", "woff2"] }] });
    if (typeof p === "string") onImportFont(p);
  }

  function setStop(i: number, patch: Partial<WatermarkConfig["stops"][number]>, commit = false) {
    set({ stops: cfg.stops.map((s, k) => (k === i ? { ...s, ...patch } : s)) }, commit);
  }

  const gradientCss = `linear-gradient(90deg, ${[...cfg.stops]
    .sort((a, b) => a.offset - b.offset)
    .map((s) => `${rgba(s.color, s.alpha)} ${s.offset * 100}%`).join(", ")})`;

  return (
    <div className="text-slate-200">
      {/* ── Préréglages ─────────────────────────────────────────── */}
      <Section title="Préréglages">
        <select value="" onChange={(e) => {
          const p = allPresets.find((x) => x.name === e.target.value);
          if (p) onLoadPreset(p);
        }} className="w-full bg-slate-800 border border-slate-700 rounded px-2 py-1 text-xs">
          <option value="">Charger un préréglage…</option>
          <optgroup label="Intégrés">
            {BUILTIN_PRESETS.map((p) => <option key={p.name}>{p.name}</option>)}
          </optgroup>
          {userPresets.length > 0 && (
            <optgroup label="Mes préréglages">
              {userPresets.map((p) => <option key={p.name}>{p.name}</option>)}
            </optgroup>
          )}
        </select>
        <div className="flex gap-1">
          <input value={presetName} onChange={(e) => setPresetName(e.target.value)} placeholder="Nom du préréglage"
            className="flex-1 min-w-0 bg-slate-800 border border-slate-700 rounded px-2 py-1 text-xs" />
          <button className={btn} disabled={!presetName.trim()}
            onClick={() => { onSavePreset(presetName.trim()); setPresetName(""); }}>Enregistrer</button>
        </div>
        {userPresets.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {userPresets.map((p) => (
              <span key={p.name} className="text-[10px] bg-slate-800 rounded px-1.5 py-0.5 flex items-center gap-1">
                {p.name}
                <button onClick={() => onDeletePreset(p.name)} className="text-slate-500 hover:text-red-400" title="Supprimer">✕</button>
              </span>
            ))}
          </div>
        )}
      </Section>

      {/* ── Contenu ─────────────────────────────────────────────── */}
      <Section title="Contenu">
        <Segmented value={cfg.kind} onChange={(v) => set({ kind: v }, true)}
          options={[{ value: "text", label: "Texte" }, { value: "image", label: "Image / logo" }]} />

        {cfg.kind === "text" ? (
          <>
            <textarea value={cfg.text} rows={2} onChange={(e) => set({ text: e.target.value })} onBlur={onCommit}
              placeholder="Texte du filigrane (Entrée = nouvelle ligne)"
              className="w-full bg-slate-800 border border-slate-700 rounded px-2 py-1 text-sm resize-y" />
            <div className="flex gap-1">
              <select value={cfg.fontFamily} onChange={(e) => set({ fontFamily: e.target.value, fontFile: null }, true)}
                style={{ fontFamily: cfg.fontFamily }}
                className="flex-1 min-w-0 bg-slate-800 border border-slate-700 rounded px-2 py-1 text-xs">
                {[...new Set([cfg.fontFamily, ...fonts, ...COMMON_FONTS])].map((f) => (
                  <option key={f} value={f} style={{ fontFamily: f }}>{f}</option>
                ))}
              </select>
              <button className={btn} onClick={pickFont} title="Importer un fichier .ttf / .otf / .woff">Importer…</button>
            </div>
            <input value={cfg.fontFamily} onChange={(e) => set({ fontFamily: e.target.value, fontFile: null })} onBlur={onCommit}
              placeholder="…ou nom de police installée"
              className="w-full bg-slate-800 border border-slate-700 rounded px-2 py-1 text-xs" />
            <div className="flex flex-wrap gap-3">
              <Toggle label="Gras" checked={cfg.bold} onChange={(v) => set({ bold: v }, true)} />
              <Toggle label="Italique" checked={cfg.italic} onChange={(v) => set({ italic: v }, true)} />
              <Toggle label="MAJUSCULES" checked={cfg.uppercase} onChange={(v) => set({ uppercase: v }, true)} />
            </div>
            <Segmented<TextAlign> value={cfg.align} onChange={(v) => set({ align: v }, true)}
              options={[{ value: "left", label: "Gauche" }, { value: "center", label: "Centre" }, { value: "right", label: "Droite" }]} />
            <Slider label="Espacement lettres" value={cfg.letterSpacing} min={-0.2} max={1.5} step={0.01} unit="em"
              onChange={(v) => set({ letterSpacing: v })} onCommit={onCommit} />
            <Slider label="Interligne" value={cfg.lineHeight} min={0.6} max={3} step={0.05}
              onChange={(v) => set({ lineHeight: v })} onCommit={onCommit} />
          </>
        ) : (
          <div className="space-y-1">
            <button className={`${btn} w-full`} onClick={pickLogo}>Choisir une image…</button>
            <p className="text-[10px] text-slate-500 break-all">{cfg.logoPath ?? "Aucune image (PNG transparent conseillé)"}</p>
          </div>
        )}
      </Section>

      {/* ── Couleur ─────────────────────────────────────────────── */}
      {cfg.kind === "text" && (
        <Section title="Couleur & dégradé">
          <Segmented<FillMode> value={cfg.fillMode} onChange={(v) => set({ fillMode: v }, true)}
            options={[
              { value: "solid", label: "Unie" }, { value: "linear", label: "Linéaire" },
              { value: "radial", label: "Radial" }, { value: "none", label: "Aucune" },
            ]} />

          {cfg.fillMode === "solid" && (
            <ColorAlpha label="Couleur" color={cfg.color} alpha={cfg.colorAlpha}
              onChange={(c, a) => set({ color: c, colorAlpha: a })} onCommit={onCommit} />
          )}

          {(cfg.fillMode === "linear" || cfg.fillMode === "radial") && (
            <>
              <div className="h-4 rounded border border-slate-700" style={{ background: gradientCss }} />
              {cfg.stops.map((s, i) => (
                <div key={i} className="space-y-1 bg-slate-800/40 rounded p-1.5">
                  <ColorAlpha label={`Arrêt ${i + 1}`} color={s.color} alpha={s.alpha}
                    onChange={(c, a) => setStop(i, { color: c, alpha: a })} onCommit={onCommit} />
                  <div className="flex items-center gap-2">
                    <div className="flex-1">
                      <Slider label="Position" value={Math.round(s.offset * 100)} min={0} max={100} unit="%"
                        onChange={(v) => setStop(i, { offset: v / 100 })} onCommit={onCommit} />
                    </div>
                    {cfg.stops.length > 2 && (
                      <button className="text-slate-500 hover:text-red-400 text-xs" title="Retirer"
                        onClick={() => set({ stops: cfg.stops.filter((_, k) => k !== i) }, true)}>✕</button>
                    )}
                  </div>
                </div>
              ))}
              <div className="flex gap-1">
                <button className={btn} onClick={() => set({
                  stops: [...cfg.stops, { offset: 0.5, color: "#facc15", alpha: 1 }],
                }, true)}>+ Couleur</button>
                <button className={btn} onClick={() => set({
                  stops: cfg.stops.map((s) => ({ ...s, offset: 1 - s.offset })),
                }, true)}>Inverser</button>
              </div>
              {cfg.fillMode === "linear" && (
                <Slider label="Angle du dégradé" value={cfg.gradientAngle} min={-180} max={180} unit="°"
                  onChange={(v) => set({ gradientAngle: v })} onCommit={onCommit} />
              )}
            </>
          )}
        </Section>
      )}

      {/* ── Contour & ombre ─────────────────────────────────────── */}
      <Section title="Contour & ombre" defaultOpen={false}>
        {cfg.kind === "text" && (
          <>
            <Slider label="Épaisseur du contour" value={cfg.strokeWidth} min={0} max={30} step={0.5} unit="%"
              onChange={(v) => set({ strokeWidth: v })} onCommit={onCommit} />
            <ColorAlpha label="Contour" color={cfg.strokeColor} alpha={cfg.strokeAlpha}
              onChange={(c, a) => set({ strokeColor: c, strokeAlpha: a })} onCommit={onCommit} />
          </>
        )}
        <Toggle label="Ombre portée" checked={cfg.shadow} onChange={(v) => set({ shadow: v }, true)} />
        {cfg.shadow && (
          <>
            <ColorAlpha label="Ombre" color={cfg.shadowColor} alpha={cfg.shadowAlpha}
              onChange={(c, a) => set({ shadowColor: c, shadowAlpha: a })} onCommit={onCommit} />
            <Slider label="Flou" value={cfg.shadowBlur} min={0} max={100} unit="%"
              onChange={(v) => set({ shadowBlur: v })} onCommit={onCommit} />
            <Slider label="Décalage X" value={cfg.shadowX} min={-50} max={50} unit="%"
              onChange={(v) => set({ shadowX: v })} onCommit={onCommit} />
            <Slider label="Décalage Y" value={cfg.shadowY} min={-50} max={50} unit="%"
              onChange={(v) => set({ shadowY: v })} onCommit={onCommit} />
          </>
        )}
      </Section>

      {/* ── Placement ───────────────────────────────────────────── */}
      <Section title="Position, taille, orientation">
        <div className="flex gap-3 items-start">
          <div className="grid grid-cols-3 gap-0.5 shrink-0" title="Ancrer">
            {[0, 0.5, 1].flatMap((ay) => [0, 0.5, 1].map((ax) => (
              <button key={`${ax}-${ay}`} onClick={() => onAnchor(ax, ay)}
                className="w-5 h-5 rounded-sm bg-slate-800 hover:bg-blue-600 border border-slate-700" />
            )))}
          </div>
          <div className="flex-1 space-y-1">
            <Toggle label="Miroir horizontal" checked={cfg.flipH} onChange={(v) => set({ flipH: v }, true)} />
            <Toggle label="Miroir vertical" checked={cfg.flipV} onChange={(v) => set({ flipV: v }, true)} />
          </div>
        </div>
        <Slider label="Taille" value={cfg.size} min={0.5} max={60} step={0.1} unit="%"
          onChange={(v) => set({ size: v })} onCommit={onCommit} />
        <Slider label="Rotation" value={cfg.rotation} min={-180} max={180} step={0.5} unit="°"
          onChange={(v) => set({ rotation: v })} onCommit={onCommit} />
        <div className="flex gap-1">
          {[-90, -45, 0, 45, 90].map((a) => (
            <button key={a} className={`${btn} flex-1`} onClick={() => set({ rotation: a }, true)}>{a}°</button>
          ))}
        </div>
        <Slider label="Opacité" value={Math.round(cfg.opacity * 100)} min={0} max={100} unit="%"
          onChange={(v) => set({ opacity: v / 100 })} onCommit={onCommit} />
        <Slider label="Position X" value={Math.round(cfg.x * 1000) / 10} min={-20} max={120} step={0.1} unit="%"
          onChange={(v) => set({ x: v / 100 })} onCommit={onCommit} />
        <Slider label="Position Y" value={Math.round(cfg.y * 1000) / 10} min={-20} max={120} step={0.1} unit="%"
          onChange={(v) => set({ y: v / 100 })} onCommit={onCommit} />
      </Section>

      {/* ── Résistance à l'effacement ───────────────────────────── */}
      <Section title="Résistance à l'effacement" defaultOpen={false}>
        <p className="text-[10px] text-slate-500 leading-relaxed">
          Aucun filigrane visible n'est indélébile face aux retouches automatiques. Ces réglages
          rendent la restauration coûteuse et salissante, et la signature invisible prouve l'origine
          même si la marque visible est effacée.
        </p>

        <button className={`${btn} w-full`} onClick={onPlaceOnBusiest}
          title="Le filigrane devient bien plus dur à effacer sur une zone riche en détails que sur un fond uni">
          Placer sur la zone la plus détaillée
        </button>

        <label className="block text-[11px] text-slate-400">
          Mode de fusion
          <select value={cfg.blend} onChange={(e) => set({ blend: e.target.value as BlendMode }, true)}
            className="w-full mt-0.5 bg-slate-800 border border-slate-700 rounded px-2 py-1 text-xs text-slate-200">
            {BLEND_MODES.map((b) => <option key={b.value} value={b.value}>{b.label}</option>)}
          </select>
        </label>
        {!blendSupported && cfg.blend !== "normal" && (
          <p className="text-[10px] text-amber-300">
            Ce format intègre le filigrane comme une image posée : le mode de fusion ne s'y applique pas
            (il reste actif pour les images, PDF, SVG et HTML).
          </p>
        )}
        {blendSupported && cfg.blend !== "normal" && (
          <p className="text-[10px] text-slate-500">
            Les pixels d'origine sont altérés, pas seulement recouverts : les redessiner devient un vrai travail de reconstruction.
          </p>
        )}

        <Slider label="Bruit dans la marque" value={cfg.noise} min={0} max={100} unit="%"
          onChange={(v) => set({ noise: v })} onCommit={onCommit} />

        <div className="pt-1 border-t border-slate-800">
          <Toggle label="Mosaïque irrégulière" checked={cfg.tile && (cfg.tileJitterSize > 0 || cfg.tileJitterAngle > 0)}
            onChange={(v) => set(v
              ? { tile: true, tileJitterSize: 45, tileJitterAngle: 35, tileJitterPos: 30 }
              : { tileJitterSize: 0, tileJitterAngle: 0, tileJitterPos: 0 }, true)} />
          <p className="text-[10px] text-slate-500 mb-1">
            Tailles et angles différents d'une répétition à l'autre : chaque zone à reconstituer devient un cas particulier.
          </p>
          {cfg.tile && (
            <>
              <Slider label="Variation de taille" value={cfg.tileJitterSize} min={0} max={80} unit="%"
                onChange={(v) => set({ tileJitterSize: v })} onCommit={onCommit} />
              <Slider label="Variation d'angle" value={cfg.tileJitterAngle} min={0} max={90} unit="°"
                onChange={(v) => set({ tileJitterAngle: v })} onCommit={onCommit} />
              <Slider label="Décalage aléatoire" value={cfg.tileJitterPos} min={0} max={60} unit="%"
                onChange={(v) => set({ tileJitterPos: v })} onCommit={onCommit} />
            </>
          )}
        </div>

        <div className="pt-1 border-t border-slate-800 space-y-1">
          <label className="block text-[11px] text-slate-400">
            Signature invisible (images)
            <input value={cfg.signature} onChange={(e) => set({ signature: e.target.value })} onBlur={onCommit}
              placeholder="© Votre nom — 2026"
              className="w-full mt-0.5 bg-slate-800 border border-slate-700 rounded px-2 py-1 text-xs text-slate-200" />
          </label>
          <p className="text-[10px] text-slate-500">
            Inscrite dans les fréquences de l'image, invisible à l'œil. Elle survit au réencodage JPEG et
            à un repeint partiel, et se relit avec « Vérifier une signature ». Les autres formats
            (PDF, documents, audio) ne la reçoivent pas.
          </p>
          <button className={`${btn} w-full`} onClick={onReadSignature}>Vérifier une signature…</button>
        </div>
      </Section>

      {/* ── Mosaïque ────────────────────────────────────────────── */}
      <Section title="Répétition (mosaïque)" defaultOpen={cfg.tile}>
        <Toggle label="Répéter sur toute la surface" checked={cfg.tile} onChange={(v) => set({ tile: v }, true)} />
        {cfg.tile && (
          <>
            <Slider label="Écart horizontal" value={cfg.tileGapX} min={0} max={80} step={0.5} unit="%"
              onChange={(v) => set({ tileGapX: v })} onCommit={onCommit} />
            <Slider label="Écart vertical" value={cfg.tileGapY} min={0} max={80} step={0.5} unit="%"
              onChange={(v) => set({ tileGapY: v })} onCommit={onCommit} />
            <Toggle label="Quinconce" checked={cfg.tileStagger} onChange={(v) => set({ tileStagger: v }, true)} />
          </>
        )}
      </Section>
    </div>
  );
}
