use anyhow::{anyhow, Result};
use std::io::Read;

// ── Protection CSV contre l'injection de formule ──────────────────────────────

pub(crate) fn csv_safe(v: &str) -> String {
    let t = v.trim_start_matches(|c| matches!(c, '\t' | '\r'));
    if t.starts_with('=') || t.starts_with('+') || t.starts_with('-') || t.starts_with('@') {
        format!("'{}", v)
    } else {
        v.to_string()
    }
}

pub(crate) fn csv_cell(v: &str) -> String {
    let safe = csv_safe(v);
    if safe.contains(',') || safe.contains('"') || safe.contains('\n') {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DOCX → texte / HTML / PDF
// ═══════════════════════════════════════════════════════════════════════════════

/// Extrait le texte brut d'un fichier DOCX.
pub fn docx_to_text(input_path: &str) -> Result<String> {
    extract_xml_text(input_path, "word/document.xml", b"w:t", b"w:p")
}

/// DOCX → HTML (texte extrait enveloppé en balises HTML basiques).
pub fn docx_to_html(input_path: &str, output_path: &str) -> Result<()> {
    let text = docx_to_text(input_path)?;
    let body: String = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| format!("<p>{}</p>", crate::pdf_engine::html_escape(l)))
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        "<!DOCTYPE html>\n<html>\n<head>\
        <meta charset=\"UTF-8\">\
        <style>body{{font-family:sans-serif;max-width:800px;margin:auto;padding:2rem;line-height:1.6}}</style>\
        </head>\n<body>\n{}\n</body>\n</html>",
        body
    );
    std::fs::write(output_path, html)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// PPTX → texte / PDF
// ═══════════════════════════════════════════════════════════════════════════════

/// Extrait le texte brut d'un fichier PPTX (toutes les slides).
pub fn pptx_to_text(input_path: &str) -> Result<String> {
    let file = std::fs::File::open(input_path)
        .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| anyhow!("Archive ZIP invalide: {}", e))?;

    let slide_count = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index(i)
                .map(|f| f.name().starts_with("ppt/slides/slide") && f.name().ends_with(".xml"))
                .unwrap_or(false)
        })
        .count();

    let mut full_text = String::new();

    for i in 1..=slide_count {
        let slide_name = format!("ppt/slides/slide{}.xml", i);
        if let Ok(mut entry) = archive.by_name(&slide_name) {
            let mut xml = String::new();
            entry.read_to_string(&mut xml).ok();
            let slide_text = parse_xml_text(&xml, b"a:t", b"a:p");
            if !slide_text.trim().is_empty() {
                full_text.push_str(&format!("--- Slide {} ---\n{}\n\n", i, slide_text.trim()));
            }
        }
    }

    if full_text.is_empty() {
        return Err(anyhow!("Aucun texte extractible dans ce PPTX"));
    }
    Ok(full_text.trim().to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Excel (XLSX, XLS, ODS) → CSV / JSON / TXT
// ═══════════════════════════════════════════════════════════════════════════════

/// Lit toutes les feuilles d'un fichier Excel et retourne les données.
fn read_excel_all_sheets(
    input_path: &str,
) -> Result<Vec<(String, Vec<Vec<String>>)>> {
    use calamine::{open_workbook_auto, Reader};

    let mut workbook = open_workbook_auto(input_path)
        .map_err(|e| anyhow!("Impossible d'ouvrir '{}': {}", input_path, e))?;

    let sheet_names = workbook.sheet_names().to_vec();
    let mut sheets = Vec::new();

    for name in &sheet_names {
        let range = workbook
            .worksheet_range(name)
            .map_err(|e| anyhow!("Feuille '{}': {}", name, e))?;

        let rows: Vec<Vec<String>> = range
            .rows()
            .map(|row| row.iter().map(|cell| cell_to_string(cell)).collect())
            .collect();

        sheets.push((name.clone(), rows));
    }

    Ok(sheets)
}

fn cell_to_string(data: &calamine::Data) -> String {
    match data {
        calamine::Data::Int(i)    => i.to_string(),
        calamine::Data::Float(f)  => {
            if f.fract() == 0.0 && f.abs() < 1e14 {
                format!("{}", *f as i64)
            } else {
                format!("{:.6}", f).trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        calamine::Data::String(s) => s.clone(),
        calamine::Data::Bool(b)   => b.to_string(),
        calamine::Data::Error(e)  => format!("#{:?}", e),
        calamine::Data::Empty     => String::new(),
        _                         => String::new(),
    }
}

/// Excel → CSV (première feuille uniquement).
pub fn excel_to_csv(input_path: &str, output_path: &str) -> Result<()> {
    let sheets = read_excel_all_sheets(input_path)?;
    let (_, rows) = sheets.into_iter().next()
        .ok_or_else(|| anyhow!("Aucune feuille trouvée"))?;

    let mut output = String::new();
    for row in &rows {
        let line: Vec<String> = row.iter().map(|v| csv_cell(v)).collect();
        output.push_str(&line.join(","));
        output.push('\n');
    }
    std::fs::write(output_path, output)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

/// Excel → JSON (toutes les feuilles, première ligne = headers).
pub fn excel_to_json(input_path: &str, output_path: &str) -> Result<()> {
    let sheets = read_excel_all_sheets(input_path)?;
    let mut result = serde_json::Map::new();

    for (sheet_name, rows) in &sheets {
        if rows.is_empty() {
            result.insert(sheet_name.clone(), serde_json::Value::Array(vec![]));
            continue;
        }
        let headers = &rows[0];
        let records: Vec<serde_json::Value> = rows[1..].iter().map(|row| {
            let obj: serde_json::Map<String, serde_json::Value> = headers
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let val = row.get(i).cloned().unwrap_or_default();
                    (h.clone(), serde_json::Value::String(val))
                })
                .collect();
            serde_json::Value::Object(obj)
        }).collect();

        result.insert(sheet_name.clone(), serde_json::Value::Array(records));
    }

    let json = serde_json::to_string_pretty(&serde_json::Value::Object(result))
        .map_err(|e| anyhow!("JSON: {}", e))?;
    std::fs::write(output_path, json)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

/// Excel → TXT (toutes les feuilles, colonnes séparées par tabulation).
pub fn excel_to_txt(input_path: &str, output_path: &str) -> Result<()> {
    let sheets = read_excel_all_sheets(input_path)?;
    let mut output = String::new();

    for (name, rows) in &sheets {
        output.push_str(&format!("=== {} ===\n", name));
        for row in rows {
            output.push_str(&row.join("\t"));
            output.push('\n');
        }
        output.push('\n');
    }
    std::fs::write(output_path, output)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// CSV → JSON / XLSX / TXT
// ═══════════════════════════════════════════════════════════════════════════════

/// CSV → JSON (première ligne = headers).
pub fn csv_to_json(input_path: &str, output_path: &str) -> Result<()> {
    let content = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| anyhow!("Headers CSV: {}", e))?
        .iter()
        .map(String::from)
        .collect();

    let mut records = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| anyhow!("Ligne CSV: {}", e))?;
        let obj: serde_json::Map<String, serde_json::Value> = headers
            .iter()
            .zip(record.iter())
            .map(|(h, v)| (h.clone(), serde_json::Value::String(v.to_string())))
            .collect();
        records.push(serde_json::Value::Object(obj));
    }

    let json = serde_json::to_string_pretty(&records)
        .map_err(|e| anyhow!("JSON: {}", e))?;
    std::fs::write(output_path, json)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

/// CSV → XLSX.
pub fn csv_to_xlsx(input_path: &str, output_path: &str) -> Result<()> {
    use rust_xlsxwriter::Workbook;

    let content = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_reader(content.as_bytes());

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();

    for (row_idx, result) in reader.records().enumerate() {
        let record = result.map_err(|e| anyhow!("Ligne CSV: {}", e))?;
        for (col_idx, value) in record.iter().enumerate() {
            sheet
                .write(row_idx as u32, col_idx as u16, value)
                .map_err(|e| anyhow!("Ecriture cellule: {}", e))?;
        }
    }

    workbook
        .save(output_path)
        .map_err(|e| anyhow!("Sauvegarde XLSX: {}", e))?;
    Ok(())
}

/// CSV → TXT (colonnes séparées par tabulation, respecte les cellules quotées).
pub fn csv_to_txt(input_path: &str, output_path: &str) -> Result<()> {
    let content = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_reader(content.as_bytes());

    let mut output = String::new();
    for result in reader.records() {
        let record = result.map_err(|e| anyhow!("Ligne CSV: {}", e))?;
        output.push_str(&record.iter().collect::<Vec<_>>().join("\t"));
        output.push('\n');
    }
    std::fs::write(output_path, output)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers XML internes
// ═══════════════════════════════════════════════════════════════════════════════

/// Extrait le texte d'une entrée spécifique dans une archive ZIP.
fn extract_xml_text(
    zip_path: &str,
    entry_name: &str,
    text_tag: &[u8],
    paragraph_tag: &[u8],
) -> Result<String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| anyhow!("Ouverture '{}': {}", zip_path, e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| anyhow!("Archive ZIP invalide: {}", e))?;

    let mut xml_content = String::new();
    archive
        .by_name(entry_name)
        .map_err(|_| anyhow!("Entrée '{}' introuvable (pas un DOCX valide ?)", entry_name))?
        .read_to_string(&mut xml_content)
        .map_err(|e| anyhow!("Lecture XML: {}", e))?;

    Ok(parse_xml_text(&xml_content, text_tag, paragraph_tag))
}

/// Parse un XML et extrait le texte des balises `text_tag`.
/// Ajoute un retour à la ligne à chaque `paragraph_tag` fermant.
fn parse_xml_text(xml: &str, text_tag: &[u8], paragraph_tag: &[u8]) -> String {
    use quick_xml::{events::Event, Reader};

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut output = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == text_tag {
                    in_text = true;
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == paragraph_tag {
                    output.push('\n');
                }
                if e.name().as_ref() == text_tag {
                    in_text = false;
                }
            }
            Ok(Event::Text(ref e)) if in_text => {
                if let Ok(s) = e.unescape() {
                    output.push_str(&s);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    output
}

