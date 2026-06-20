use crate::conversion_engine::{
    build_output_path_custom, convert_image_file, convert_svg_to_image,
    generate_thumbnail, get_available_formats, svg_to_dynamic_image, ImageOptions, OutputFormat,
};
use crate::office_engine::{
    csv_to_json, csv_to_txt, csv_to_xlsx,
    docx_to_html, docx_to_text,
    excel_to_csv, excel_to_json, excel_to_txt,
    pptx_to_text,
};
use crate::pdf_engine::{
    extract_text_from_pdf, pdf_to_html,
    get_pdf_page_count as pdf_page_count,
    images_to_pdf, merge_pdfs, merge_pdfs_pages, merge_pdfs_single_page, split_pdf,
};
use crate::text_engine::{
    create_pdf_from_text, html_to_pdf, html_to_txt,
    md_to_html, md_to_pdf, md_to_txt, txt_to_pdf,
};

// ── Taille maximale de fichier acceptée ───────────────────────────────────────

const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB

// ── Validation du chemin de sortie (anti path traversal + symlink) ────────────

fn validate_output_path(path: &str) -> Result<(), String> {
    use std::path::Component;

    let p = std::path::Path::new(path);

    // Bloquer les chemins UNC (\\server\share)
    let raw = p.to_string_lossy();
    if raw.starts_with("\\\\") || raw.starts_with("//") {
        return Err("Chemin UNC refusé : écriture sur des partages réseau interdite".to_string());
    }

    // Bloquer les séquences '..'
    for component in p.components() {
        if matches!(component, Component::ParentDir) {
            return Err("Chemin de sortie invalide : séquence '..' interdite".to_string());
        }
    }

    // Canonicaliser via le répertoire parent pour détecter les symlinks/jonctions
    if let Some(parent) = p.parent() {
        // Créer le répertoire parent si nécessaire (cas output_dir personnalisé)
        if !parent.exists() {
            // Tentative silencieuse de création; l'échec sera géré à l'écriture
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(canonical) = parent.canonicalize() {
            let s = canonical.to_string_lossy().to_lowercase();
            let forbidden = [
                "c:\\windows",
                "c:\\program files",
                "c:\\program files (x86)",
                "c:\\programdata",
                "c:\\system",
            ];
            if forbidden.iter().any(|f| s.starts_with(f)) {
                return Err(format!(
                    "Chemin refusé : écriture interdite dans une zone système protégée ({})",
                    canonical.display()
                ));
            }
        }
    }

    Ok(())
}

// ── Validation d'un répertoire de sortie personnalisé ─────────────────────────

fn validate_output_dir(dir: &str) -> Result<(), String> {
    use std::path::Component;
    let p = std::path::Path::new(dir);
    let raw = p.to_string_lossy();

    if raw.starts_with("\\\\") || raw.starts_with("//") {
        return Err("Répertoire UNC refusé".to_string());
    }
    for component in p.components() {
        if matches!(component, Component::ParentDir) {
            return Err("Répertoire de sortie invalide : séquence '..' interdite".to_string());
        }
    }
    Ok(())
}

// ── Chemin de fichier temporaire unique (temp dir système) ────────────────────

fn unique_tmp(ext: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("uc_{}_{}.tmp", ts, ext))
        .to_string_lossy()
        .to_string()
}

// ── RAII : suppression garantie des fichiers temporaires ──────────────────────

struct TempFile(String);
impl Drop for TempFile {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

// ── Résultat de conversion ─────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct ConversionResult {
    pub path: String,
    pub output_size: u64,
}

// ── Conversion unifiée ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn convert_file(
    input_path: String,
    output_format: String,
    output_dir: Option<String>,
    output_name: Option<String>,
    quality: Option<u8>,
    resize_width: Option<u32>,
    resize_height: Option<u32>,
    rotation: Option<u32>,
) -> Result<ConversionResult, String> {
    // ── Validation du répertoire de sortie personnalisé ──────────────────────
    if let Some(ref dir) = output_dir {
        validate_output_dir(dir)?;
    }

    // ── Validation du nom de sortie (interdire '/' '\\' '..' et null bytes) ─
    if let Some(ref name) = output_name {
        if name.contains('\0') || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("Nom de sortie invalide : caractères interdits détectés".to_string());
        }
    }

    // ── Vérification taille fichier ───────────────────────────────────────────
    let file_size = std::fs::metadata(&input_path)
        .map(|m| m.len())
        .map_err(|e| format!("Fichier introuvable '{}': {}", input_path, e))?;
    if file_size > MAX_FILE_SIZE {
        return Err(format!(
            "Fichier trop volumineux ({:.1} MB). Limite : {} MB.",
            file_size as f64 / (1024.0 * 1024.0),
            MAX_FILE_SIZE / (1024 * 1024)
        ));
    }

    let ext = std::path::Path::new(&input_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let fmt = output_format.to_lowercase();

    let out = build_output_path_custom(
        &input_path, &fmt,
        output_dir.as_deref(),
        output_name.as_deref(),
    );

    // Valider le chemin de sortie final construit
    validate_output_path(&out)?;

    let img_opts = ImageOptions {
        quality,
        resize_width,
        resize_height,
        rotation,
    };

    match (ext.as_str(), fmt.as_str()) {

        // ── Images raster → image ─────────────────────────────────────────────
        (img, f)
            if matches!(img, "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tif"|"tga"|"pnm"|"hdr"|"ico")
            && matches!(f, "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tga"|"ico") =>
        {
            let format = OutputFormat::from_str(f).map_err(|e| e.to_string())?;
            convert_image_file(&input_path, &out, &format, &img_opts).map_err(|e| e.to_string())?;
        }

        // ── Images raster → PDF ───────────────────────────────────────────────
        (img, "pdf")
            if matches!(img, "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tif"|"tga"|"pnm"|"hdr"|"ico") =>
        {
            images_to_pdf(&[input_path.clone()], &out).map_err(|e| e.to_string())?;
        }

        // ── SVG → image raster ────────────────────────────────────────────────
        ("svg", f) if matches!(f, "png"|"jpg"|"jpeg"|"webp"|"bmp") => {
            let format = OutputFormat::from_str(f).map_err(|e| e.to_string())?;
            convert_svg_to_image(&input_path, &out, &format, &img_opts).map_err(|e| e.to_string())?;
        }

        // ── SVG → PDF ─────────────────────────────────────────────────────────
        ("svg", "pdf") => {
            let img = svg_to_dynamic_image(&input_path).map_err(|e| e.to_string())?;
            let tmp = unique_tmp("png");
            let _guard = TempFile(tmp.clone());
            img.save_with_format(&tmp, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            images_to_pdf(&[tmp.clone()], &out).map_err(|e| e.to_string())?;
        }

        // ── PDF → TXT ─────────────────────────────────────────────────────────
        ("pdf", "txt") => {
            let text = extract_text_from_pdf(&input_path).map_err(|e| e.to_string())?;
            std::fs::write(&out, text).map_err(|e| e.to_string())?;
        }

        // ── PDF → HTML ────────────────────────────────────────────────────────
        ("pdf", "html") => {
            pdf_to_html(&input_path, &out).map_err(|e| e.to_string())?;
        }

        // ── TXT → PDF ─────────────────────────────────────────────────────────
        ("txt", "pdf") => { txt_to_pdf(&input_path, &out).map_err(|e| e.to_string())?; }

        // ── Markdown ──────────────────────────────────────────────────────────
        ("md"|"markdown", "html") => { md_to_html(&input_path, &out).map_err(|e| e.to_string())?; }
        ("md"|"markdown", "txt")  => { md_to_txt(&input_path, &out).map_err(|e| e.to_string())?; }
        ("md"|"markdown", "pdf")  => { md_to_pdf(&input_path, &out).map_err(|e| e.to_string())?; }

        // ── HTML ──────────────────────────────────────────────────────────────
        ("html"|"htm", "txt") => { html_to_txt(&input_path, &out).map_err(|e| e.to_string())?; }
        ("html"|"htm", "pdf") => { html_to_pdf(&input_path, &out).map_err(|e| e.to_string())?; }

        // ── DOCX / DOC ────────────────────────────────────────────────────────
        ("docx"|"doc", "txt") => {
            let text = docx_to_text(&input_path).map_err(|e| e.to_string())?;
            std::fs::write(&out, &text).map_err(|e| e.to_string())?;
        }
        ("docx"|"doc", "html") => { docx_to_html(&input_path, &out).map_err(|e| e.to_string())?; }
        ("docx"|"doc", "pdf") => {
            let text = docx_to_text(&input_path).map_err(|e| e.to_string())?;
            create_pdf_from_text(&text, &out).map_err(|e| e.to_string())?;
        }

        // ── PPTX / PPT ────────────────────────────────────────────────────────
        ("pptx"|"ppt", "txt") => {
            let text = pptx_to_text(&input_path).map_err(|e| e.to_string())?;
            std::fs::write(&out, &text).map_err(|e| e.to_string())?;
        }
        ("pptx"|"ppt", "pdf") => {
            let text = pptx_to_text(&input_path).map_err(|e| e.to_string())?;
            create_pdf_from_text(&text, &out).map_err(|e| e.to_string())?;
        }

        // ── Excel ─────────────────────────────────────────────────────────────
        ("xlsx"|"xls"|"ods", "csv")  => { excel_to_csv(&input_path, &out).map_err(|e| e.to_string())?; }
        ("xlsx"|"xls"|"ods", "json") => { excel_to_json(&input_path, &out).map_err(|e| e.to_string())?; }
        ("xlsx"|"xls"|"ods", "txt")  => { excel_to_txt(&input_path, &out).map_err(|e| e.to_string())?; }
        ("xlsx"|"xls"|"ods", "pdf")  => {
            let tmp = unique_tmp("txt");
            let _guard = TempFile(tmp.clone());
            excel_to_txt(&input_path, &tmp).map_err(|e| e.to_string())?;
            txt_to_pdf(&tmp, &out).map_err(|e| e.to_string())?;
        }

        // ── CSV ───────────────────────────────────────────────────────────────
        ("csv", "json") => { csv_to_json(&input_path, &out).map_err(|e| e.to_string())?; }
        ("csv", "xlsx") => { csv_to_xlsx(&input_path, &out).map_err(|e| e.to_string())?; }
        ("csv", "txt")  => { csv_to_txt(&input_path, &out).map_err(|e| e.to_string())?; }
        ("csv", "pdf")  => {
            let tmp = unique_tmp("txt");
            let _guard = TempFile(tmp.clone());
            csv_to_txt(&input_path, &tmp).map_err(|e| e.to_string())?;
            txt_to_pdf(&tmp, &out).map_err(|e| e.to_string())?;
        }

        // ── JSON ──────────────────────────────────────────────────────────────
        ("json", "csv") => {
            let json_str = std::fs::read_to_string(&input_path).map_err(|e| e.to_string())?;
            json_to_csv_str(&json_str, &out).map_err(|e| e.to_string())?;
        }
        ("json", "txt") => {
            let raw = std::fs::read_to_string(&input_path).map_err(|e| e.to_string())?;
            let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            let pretty = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
            std::fs::write(&out, pretty).map_err(|e| e.to_string())?;
        }

        _ => {
            return Err(format!("Conversion .{} → {} non supportée", ext, fmt));
        }
    }

    let output_size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    Ok(ConversionResult { path: out, output_size })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn json_to_csv_str(json_str: &str, output_path: &str) -> anyhow::Result<()> {
    use anyhow::anyhow;
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("JSON invalide: {}", e))?;
    let records = value.as_array()
        .ok_or_else(|| anyhow!("JSON doit être un tableau d'objets"))?;

    if records.is_empty() {
        std::fs::write(output_path, "")?;
        return Ok(());
    }

    let headers: Vec<String> = records[0]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    let mut csv_output = headers.iter().map(|h| crate::office_engine::csv_cell(h)).collect::<Vec<_>>().join(",");
    csv_output.push('\n');

    for record in records {
        if let Some(obj) = record.as_object() {
            let row: Vec<String> = headers.iter().map(|h| {
                let v = obj.get(h).map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                }).unwrap_or_default();
                crate::office_engine::csv_cell(&v)
            }).collect();
            csv_output.push_str(&row.join(","));
            csv_output.push('\n');
        }
    }
    std::fs::write(output_path, csv_output)
        .map_err(|e| anyhow!("Ecriture CSV: {}", e))?;
    Ok(())
}

// ── Commandes utilitaires ─────────────────────────────────────────────────────

#[tauri::command]
pub fn get_formats_for_extension(input_ext: String) -> Vec<&'static str> {
    get_available_formats(&input_ext)
}

#[tauri::command]
pub async fn merge_images_to_pdf(
    image_paths: Vec<String>,
    output_path: String,
) -> Result<String, String> {
    validate_output_path(&output_path)?;
    // Vérifier la taille de chaque image source
    for img_path in &image_paths {
        let size = std::fs::metadata(img_path)
            .map(|m| m.len())
            .map_err(|e| format!("Image introuvable '{}': {}", img_path, e))?;
        if size > MAX_FILE_SIZE {
            return Err(format!(
                "Image trop volumineuse ({:.1} MB) : '{}'. Limite : {} MB.",
                size as f64 / (1024.0 * 1024.0),
                img_path,
                MAX_FILE_SIZE / (1024 * 1024)
            ));
        }
    }
    images_to_pdf(&image_paths, &output_path).map_err(|e| e.to_string())?;
    Ok(output_path)
}

#[tauri::command]
pub async fn get_thumbnail(input_path: String) -> Result<String, String> {
    let size = std::fs::metadata(&input_path)
        .map(|m| m.len())
        .map_err(|e| e.to_string())?;
    if size > MAX_FILE_SIZE {
        return Err(format!("Fichier trop volumineux pour la miniature ({:.1} MB)", size as f64 / (1024.0 * 1024.0)));
    }
    generate_thumbnail(&input_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_file_size(path: String) -> Result<u64, String> {
    std::fs::metadata(&path)
        .map(|m| m.len())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_pdf_page_count(input_path: String) -> Result<u32, String> {
    pdf_page_count(&input_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn split_pdf_command(
    input_path: String,
    pages: Vec<u32>,
    output_path: String,
) -> Result<String, String> {
    validate_output_path(&output_path)?;
    if pages.is_empty() {
        return Err("Aucune page sélectionnée".to_string());
    }
    let total = pdf_page_count(&input_path).map_err(|e| e.to_string())?;
    for &p in &pages {
        if p < 1 || p > total {
            return Err(format!("Page {} hors limites (1–{})", p, total));
        }
    }
    split_pdf(&input_path, &pages, &output_path).map_err(|e| e.to_string())?;
    Ok(output_path)
}

#[tauri::command]
pub async fn merge_pdfs_command(
    input_paths: Vec<String>,
    output_path: String,
) -> Result<String, String> {
    validate_output_path(&output_path)?;
    merge_pdfs(&input_paths, &output_path).map_err(|e| e.to_string())?;
    Ok(output_path)
}

/// Mode "pages" : fusion lopdf (pages réelles préservées).
/// Mode "single" : page unique haute (texte extrait condensé).
#[tauri::command]
pub async fn merge_pdfs_mode_command(
    input_paths: Vec<String>,
    output_path: String,
    mode: String,
) -> Result<String, String> {
    validate_output_path(&output_path)?;
    match mode.as_str() {
        "pages"  => merge_pdfs_pages(&input_paths, &output_path).map_err(|e| e.to_string())?,
        "single" => merge_pdfs_single_page(&input_paths, &output_path).map_err(|e| e.to_string())?,
        _        => return Err(format!("Mode inconnu: {}", mode)),
    }
    Ok(output_path)
}

#[tauri::command]
pub async fn zip_files_command(
    paths: Vec<String>,
    output_path: String,
) -> Result<String, String> {
    validate_output_path(&output_path)?;
    zip_files(&paths, &output_path).map_err(|e| e.to_string())?;
    Ok(output_path)
}

fn zip_files(paths: &[String], output_path: &str) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use std::io::Write;
    use zip::write::FileOptions;

    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow::anyhow!("Création ZIP '{}': {}", output_path, e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: FileOptions<()> = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut used_names: HashSet<String> = HashSet::new();

    for path in paths {
        let original = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Nom de fichier invalide: {}", path))?;

        // Déduplication : ajoute _2, _3… si doublon
        let mut entry_name = original.to_string();
        let mut counter = 2u32;
        while used_names.contains(&entry_name) {
            if let Some(dot) = original.rfind('.') {
                entry_name = format!("{}_{}.{}", &original[..dot], counter, &original[dot+1..]);
            } else {
                entry_name = format!("{}_{}", original, counter);
            }
            counter += 1;
        }
        used_names.insert(entry_name.clone());

        zip.start_file(&entry_name, options)
            .map_err(|e| anyhow::anyhow!("ZIP start_file '{}': {}", entry_name, e))?;
        if !std::path::Path::new(path).exists() {
            return Err(anyhow::anyhow!(
                "Fichier introuvable: '{}'. Il a peut-être été déplacé ou la conversion n'a pas créé le fichier.",
                path
            ));
        }
        let data = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("Lecture '{}': {}", path, e))?;
        zip.write_all(&data)
            .map_err(|e| anyhow::anyhow!("ZIP write: {}", e))?;
    }
    zip.finish().map_err(|e| anyhow::anyhow!("ZIP finish: {}", e))?;
    Ok(())
}
