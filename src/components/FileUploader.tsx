import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { FileItem, IMAGE_EXTENSIONS, defaultImageOptions } from "../types";

const MEMORY_KEY = "uc_default_formats";

function getDefaultFormats(): Record<string, string> {
  try { return JSON.parse(localStorage.getItem(MEMORY_KEY) ?? "{}"); } catch { return {}; }
}
export function saveDefaultFormat(ext: string, fmt: string) {
  const mem = getDefaultFormats();
  mem[ext] = fmt;
  localStorage.setItem(MEMORY_KEY, JSON.stringify(mem));
}

interface Props {
  onFilesAdded: (files: FileItem[]) => void;
}

export function FileUploader({ onFilesAdded }: Props) {
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    const webview = getCurrentWebviewWindow();
    let unlisten: (() => void) | null = null;
    webview.onDragDropEvent(async (event) => {
      if (event.payload.type === "enter") setIsDragging(true);
      else if (event.payload.type === "leave") setIsDragging(false);
      else if (event.payload.type === "drop") {
        setIsDragging(false);
        const paths: string[] = (event.payload as { type: "drop"; paths: string[] }).paths ?? [];
        await handlePaths(paths);
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  async function handlePaths(paths: string[]) {
    const memory = getDefaultFormats();
    const items: FileItem[] = await Promise.all(
      paths.map(async (p) => {
        const name = p.split(/[\\/]/).pop() ?? p;
        const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
        let formats: string[] = [];
        try { formats = await invoke<string[]>("get_formats_for_extension", { inputExt: ext }); } catch { }

        // Format par défaut : mémorisé > premier format différent de l'entrée
        const memorized = memory[ext];
        const defaultFmt = (memorized && memorized !== ext && formats.includes(memorized))
          ? memorized
          : (formats.find((f) => f !== ext) ?? formats[0] ?? "");

        // Taille fichier
        let fileSize: number | undefined;
        try { fileSize = await invoke<number>("get_file_size", { path: p }); } catch { }

        // Miniature (images seulement)
        let thumbnail: string | undefined;
        if (IMAGE_EXTENSIONS.has(ext)) {
          try { thumbnail = await invoke<string>("get_thumbnail", { inputPath: p }); } catch { }
        }

        // Nombre de pages (PDF)
        let pageCount: number | undefined;
        if (ext === "pdf") {
          try { pageCount = await invoke<number>("get_pdf_page_count", { inputPath: p }); } catch { }
        }

        return {
          id: crypto.randomUUID(),
          name, path: p, extension: ext,
          availableFormats: formats,
          selectedFormat: defaultFmt,
          status: "idle" as const,
          progress: 0,
          fileSize, thumbnail, pageCount,
          imageOptions: defaultImageOptions(),
          outputName: "",
          showOptions: false,
        };
      })
    );
    onFilesAdded(items.filter((f) => f.availableFormats.length > 0));
  }

  async function handleBrowseClick() {
    const selected = await open({
      multiple: true,
      filters: [
        {
          name: "Fichiers supportés",
          extensions: [
            "png","jpg","jpeg","webp","bmp","gif","tiff","tif","tga","pnm","hdr","ico","svg",
            "pdf","txt","md","markdown","html","htm",
            "docx","doc","pptx","ppt",
            "xlsx","xls","ods","csv","json",
          ],
        },
        { name: "Tous les fichiers", extensions: ["*"] },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    await handlePaths(paths);
  }

  return (
    <div
      className={`
        flex flex-col items-center justify-center
        w-full h-48 rounded-2xl border-2 border-dashed
        transition-all duration-200 cursor-pointer select-none
        ${isDragging
          ? "border-blue-400 bg-blue-950/40 scale-[1.01]"
          : "border-slate-600 bg-slate-800/40 hover:border-slate-400"
        }
      `}
      onClick={handleBrowseClick}
    >
      <svg className="w-10 h-10 mb-2 text-slate-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5}
          d="M12 16v-8m0 0-3 3m3-3 3 3M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1" />
      </svg>
      <p className="text-slate-300 font-medium">
        {isDragging ? "Relâchez les fichiers ici" : "Cliquez ou déposez vos fichiers"}
      </p>
      <p className="text-slate-500 text-sm mt-1">
        Images · PDF · Word · Excel · PowerPoint · CSV · JSON
      </p>
    </div>
  );
}
