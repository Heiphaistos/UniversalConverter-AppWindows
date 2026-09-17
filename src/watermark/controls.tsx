// Petits contrôles réutilisés par le panneau du studio.

import { ReactNode, useState } from "react";

export function Section({ title, children, defaultOpen = true }: {
  title: string; children: ReactNode; defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="border-b border-slate-800">
      <button onClick={() => setOpen(!open)}
        className="w-full flex items-center justify-between px-3 py-2 text-[11px] font-semibold uppercase tracking-wide text-slate-400 hover:text-slate-200">
        {title}
        <span className="text-slate-600">{open ? "−" : "+"}</span>
      </button>
      {open && <div className="px-3 pb-3 space-y-2">{children}</div>}
    </div>
  );
}

export function Slider({ label, value, min, max, step = 1, unit = "", onChange, onCommit }: {
  label: string; value: number; min: number; max: number; step?: number; unit?: string;
  onChange: (v: number) => void; onCommit?: () => void;
}) {
  return (
    <label className="block">
      <div className="flex justify-between text-[11px] text-slate-400 mb-0.5">
        <span>{label}</span>
        <input type="number" value={Number(value.toFixed(3))} step={step}
          onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v)) onChange(v); }}
          onBlur={onCommit}
          className="w-16 bg-transparent text-right text-slate-200 outline-none" />
        {unit && <span className="text-slate-500 ml-0.5">{unit}</span>}
      </div>
      <input type="range" min={min} max={max} step={step} value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        onPointerUp={onCommit} onKeyUp={onCommit}
        className="w-full accent-blue-500" />
    </label>
  );
}

export function ColorAlpha({ label, color, alpha, onChange, onCommit }: {
  label: string; color: string; alpha: number;
  onChange: (color: string, alpha: number) => void; onCommit?: () => void;
}) {
  return (
    <div className="flex items-center gap-2 text-[11px] text-slate-400">
      <span className="w-16 shrink-0">{label}</span>
      <input type="color" value={color} onChange={(e) => onChange(e.target.value, alpha)} onBlur={onCommit}
        className="w-8 h-6 rounded bg-transparent border border-slate-700 cursor-pointer" />
      <input type="text" value={color} onChange={(e) => onChange(e.target.value, alpha)} onBlur={onCommit}
        className="w-16 shrink-0 bg-slate-800 rounded px-1 py-0.5 text-slate-200 font-mono" />
      <input type="range" min={0} max={1} step={0.01} value={alpha} title="Opacité de la couleur"
        onChange={(e) => onChange(color, parseFloat(e.target.value))} onPointerUp={onCommit}
        className="flex-1 min-w-0 accent-blue-500" />
    </div>
  );
}

export function Toggle({ label, checked, onChange }: {
  label: string; checked: boolean; onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-[11px] text-slate-300 cursor-pointer select-none">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="accent-blue-500" />
      {label}
    </label>
  );
}

export function Segmented<T extends string>({ value, options, onChange }: {
  value: T; options: { value: T; label: string }[]; onChange: (v: T) => void;
}) {
  return (
    <div className="flex bg-slate-800 rounded-lg p-0.5">
      {options.map((o) => (
        <button key={o.value} onClick={() => onChange(o.value)}
          className={`flex-1 text-[11px] py-1 rounded-md transition-colors ${
            value === o.value ? "bg-blue-600 text-white" : "text-slate-400 hover:text-slate-200"}`}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export const btn = "text-[11px] bg-slate-800 hover:bg-slate-700 border border-slate-700 rounded-md px-2 py-1 text-slate-300 transition-colors";
