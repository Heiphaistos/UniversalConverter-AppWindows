use anyhow::{anyhow, Result};
use lopdf::Document;
use printpdf::{Image, ImageTransform, ImageXObject, Mm, PdfDocument, Px};
use printpdf::{ColorBits, ColorSpace};
use std::fs::File;
use std::io::BufWriter;

// ── PDF → Texte ────────────────────────────────────────────────────────────────

pub fn extract_text_from_pdf(pdf_path: &str) -> Result<String> {
    use lopdf::content::Content;

    let doc = Document::load(pdf_path)
        .map_err(|e| anyhow!("Ouverture PDF '{}': {}", pdf_path, e))?;

    let pages = doc.get_pages();
    let mut full_text = String::new();
    let mut has_text = false;

    for (page_num, page_id) in &pages {
        let raw = match doc.get_page_content(*page_id) {
            Ok(b) => b,
            Err(_) => {
                full_text.push_str(&format!("--- Page {} [contenu inaccessible] ---\n\n", page_num));
                continue;
            }
        };
        let content = match Content::decode(&raw) {
            Ok(c) => c,
            Err(_) => {
                full_text.push_str(&format!("--- Page {} [décodage impossible] ---\n\n", page_num));
                continue;
            }
        };
        let page_text = extract_page_text(&content.operations);
        if page_text.trim().is_empty() {
            full_text.push_str(&format!("--- Page {} [aucun texte] ---\n\n", page_num));
        } else {
            has_text = true;
            full_text.push_str(&format!("--- Page {} ---\n{}\n\n", page_num, page_text.trim()));
        }
    }

    if !has_text {
        return Err(anyhow!(
            "Aucun texte extractible. Le PDF est peut-être scanné (images uniquement, OCR requis) ou vide."
        ));
    }
    Ok(full_text)
}

fn extract_page_text(operations: &[lopdf::content::Operation]) -> String {
    use lopdf::Object;

    let mut result  = String::new();
    let mut line    = String::new();
    let mut last_y: Option<f64> = None;

    for op in operations {
        match op.operator.as_ref() {
            // Matrice texte absolue [a b c d tx ty] — ty = position Y
            "Tm" if op.operands.len() == 6 => {
                let ty = obj_to_f64(&op.operands[5]);
                if let (Some(ty), Some(prev_y)) = (ty, last_y) {
                    if (ty - prev_y).abs() > 1.0 {
                        flush_line(&mut result, &mut line);
                    }
                }
                if let Some(ty) = ty { last_y = Some(ty); }
            }
            // Déplacement relatif → nouvelle ligne
            "Td" | "TD" | "T*" => flush_line(&mut result, &mut line),
            // Afficher chaîne
            "Tj" => {
                if let Some(Object::String(bytes, _)) = op.operands.first() {
                    line.push_str(&pdf_bytes_to_string(bytes));
                }
            }
            // Afficher tableau (texte kerné)
            "TJ" => {
                if let Some(Object::Array(arr)) = op.operands.first() {
                    for item in arr {
                        match item {
                            Object::String(bytes, _) => line.push_str(&pdf_bytes_to_string(bytes)),
                            Object::Integer(n) if *n < -100 => line.push(' '),
                            Object::Real(f)    if *f < -100.0 => line.push(' '),
                            _ => {}
                        }
                    }
                }
            }
            // Aller à la ligne + afficher
            "'" => {
                flush_line(&mut result, &mut line);
                if let Some(Object::String(bytes, _)) = op.operands.first() {
                    line.push_str(&pdf_bytes_to_string(bytes));
                }
            }
            "\"" => {
                flush_line(&mut result, &mut line);
                if let Some(Object::String(bytes, _)) = op.operands.last() {
                    line.push_str(&pdf_bytes_to_string(bytes));
                }
            }
            _ => {}
        }
    }
    flush_line(&mut result, &mut line);
    result
}

fn flush_line(result: &mut String, line: &mut String) {
    let t = line.trim();
    if !t.is_empty() {
        result.push_str(t);
        result.push('\n');
    }
    line.clear();
}

fn obj_to_f64(obj: &lopdf::Object) -> Option<f64> {
    match obj {
        lopdf::Object::Real(f)    => Some((*f).into()),
        lopdf::Object::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

fn pdf_bytes_to_string(bytes: &[u8]) -> String {
    // UTF-16 BE (BOM: FE FF)
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let chars: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&chars).to_string();
    }
    // UTF-8
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // Latin-1 / PDFDocEncoding
    bytes.iter().map(|&b| b as char).collect()
}

// ── PDF → HTML ─────────────────────────────────────────────────────────────────

pub fn pdf_to_html(pdf_path: &str, output_path: &str) -> Result<()> {
    let text = extract_text_from_pdf(pdf_path)?;
    let body: String = text
        .lines()
        .map(|l| {
            if l.starts_with("--- Page") {
                format!("<h2>{}</h2>", html_escape(l))
            } else if l.trim().is_empty() {
                String::new()
            } else {
                format!("<p>{}</p>", html_escape(l))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"UTF-8\">\
        <style>body{{font-family:sans-serif;max-width:800px;margin:auto;padding:2rem;line-height:1.6}}\
        h2{{color:#555;border-top:1px solid #ccc;padding-top:1em;margin-top:1.5em}}</style>\
        </head>\n<body>\n{}\n</body>\n</html>",
        body
    );
    std::fs::write(output_path, html)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Nombre de pages d'un PDF ───────────────────────────────────────────────────

pub fn get_pdf_page_count(pdf_path: &str) -> Result<u32> {
    let doc = Document::load(pdf_path)
        .map_err(|e| anyhow!("Ouverture PDF '{}': {}", pdf_path, e))?;
    Ok(doc.get_pages().len() as u32)
}

// ── Split PDF : garder seulement les pages indiquées ──────────────────────────

pub fn split_pdf(input_path: &str, pages_to_keep: &[u32], output_path: &str) -> Result<()> {
    let mut doc = Document::load(input_path)
        .map_err(|e| anyhow!("Ouverture PDF '{}': {}", input_path, e))?;

    let total = doc.get_pages().len() as u32;
    let keep_set: std::collections::HashSet<u32> = pages_to_keep.iter().cloned().collect();
    let to_delete: Vec<u32> = (1..=total).filter(|p| !keep_set.contains(p)).collect();

    if !to_delete.is_empty() {
        doc.delete_pages(&to_delete);
    }

    doc.save(output_path)
        .map_err(|e| anyhow!("Sauvegarde PDF divisé: {}", e))?;
    Ok(())
}

// ── Merge PDFs (concaténation texte) ──────────────────────────────────────────

pub fn merge_pdfs(input_paths: &[String], output_path: &str) -> Result<()> {
    if input_paths.is_empty() {
        return Err(anyhow!("Aucun PDF fourni"));
    }
    let mut combined = String::new();
    for path in input_paths {
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path.as_str());
        combined.push_str(&format!("═══ {} ═══\n\n", name));
        match extract_text_from_pdf(path) {
            Ok(text) => combined.push_str(&text),
            Err(e)   => combined.push_str(&format!("[Extraction impossible: {}]\n", e)),
        }
        combined.push_str("\n\n");
    }
    crate::text_engine::create_pdf_from_text(&combined, output_path)
}

// ── Fusion PDF (pages réelles, lopdf) ─────────────────────────────────────────

/// Fusionne plusieurs PDFs en copiant les pages réelles (layout + images conservés).
pub fn merge_pdfs_pages(input_paths: &[String], output_path: &str) -> Result<()> {
    use lopdf::Object;

    if input_paths.is_empty() {
        return Err(anyhow!("Aucun PDF fourni"));
    }

    let mut result = Document::load(&input_paths[0])
        .map_err(|e| anyhow!("Ouverture '{}': {}", &input_paths[0], e))?;

    let result_pages_id = get_pages_root_id(&result)?;

    for path in &input_paths[1..] {
        let src = Document::load(path)
            .map_err(|e| anyhow!("Ouverture '{}': {}", path, e))?;

        let src_page_ids: Vec<_> = src.get_pages().values().cloned().collect();
        let src_max_id = src.max_id;
        let offset = result.max_id;

        // Vérifier l'overflow avant de renommer les objets
        result.max_id = result.max_id.checked_add(src_max_id)
            .ok_or_else(|| anyhow!("Overflow max_id: PDFs trop volumineux pour la fusion"))?;

        // Renumber + ajouter tous les objets sources dans result
        // Sûr : id.0 <= src_max_id et offset + src_max_id <= u32::MAX (vérifié ci-dessus)
        for (id, obj) in src.objects {
            result.objects.insert((id.0 + offset, id.1), renumber_refs(obj, offset));
        }

        let new_page_ids: Vec<_> = src_page_ids.iter().map(|id| (id.0 + offset, id.1)).collect();

        // Mettre à jour Parent de chaque page ajoutée
        for &page_id in &new_page_ids {
            if let Some(Object::Dictionary(ref mut d)) = result.objects.get_mut(&page_id) {
                d.set(b"Parent", Object::Reference(result_pages_id));
            }
        }

        // Cloner le nœud Pages, mettre à jour Kids + Count, réinsérer
        if let Some(pages_obj) = result.objects.get(&result_pages_id).cloned() {
            if let Object::Dictionary(mut pages_dict) = pages_obj {
                let current_count = pages_dict.get(b"Count").ok()
                    .and_then(|c| c.as_i64().ok())
                    .unwrap_or(0);

                let mut new_kids: Vec<Object> = match pages_dict.get(b"Kids") {
                    Ok(Object::Array(arr)) => arr.clone(),
                    _ => vec![],
                };
                for &pid in &new_page_ids {
                    new_kids.push(Object::Reference(pid));
                }
                pages_dict.set(b"Kids", Object::Array(new_kids));
                pages_dict.set(b"Count", Object::Integer(current_count + new_page_ids.len() as i64));

                result.objects.insert(result_pages_id, Object::Dictionary(pages_dict));
            }
        }
    }

    result.save(output_path)
        .map_err(|e| anyhow!("Sauvegarde PDF fusionné: {}", e))?;
    Ok(())
}

fn get_pages_root_id(doc: &Document) -> Result<lopdf::ObjectId> {
    use lopdf::Object;
    let cat_ref = doc.trailer.get(b"Root")
        .map_err(|_| anyhow!("Catalog introuvable"))?;
    let cat_id = if let Object::Reference(id) = cat_ref { *id }
                 else { return Err(anyhow!("Root non-reference")); };
    let cat = doc.get_object(cat_id)
        .map_err(|_| anyhow!("Catalog objet introuvable"))?;
    let pages_ref = cat.as_dict()
        .map_err(|_| anyhow!("Catalog non-dict"))?
        .get(b"Pages")
        .map_err(|_| anyhow!("Pages absent du catalog"))?;
    if let Object::Reference(id) = pages_ref { Ok(*id) }
    else { Err(anyhow!("Pages non-reference")) }
}

fn renumber_refs(obj: lopdf::Object, offset: u32) -> lopdf::Object {
    use lopdf::Object;
    match obj {
        Object::Reference(id) => Object::Reference((id.0 + offset, id.1)),
        Object::Array(arr) => Object::Array(
            arr.into_iter().map(|o| renumber_refs(o, offset)).collect()
        ),
        Object::Dictionary(dict) => {
            let mut new_dict = lopdf::Dictionary::new();
            for (k, v) in dict.iter() {
                new_dict.set(k.clone(), renumber_refs(v.clone(), offset));
            }
            Object::Dictionary(new_dict)
        }
        Object::Stream(mut stream) => {
            let pairs: Vec<(Vec<u8>, lopdf::Object)> = stream.dict.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            stream.dict = lopdf::Dictionary::new();
            for (k, v) in pairs {
                stream.dict.set(k, renumber_refs(v, offset));
            }
            Object::Stream(stream)
        }
        other => other,
    }
}

// ── Fusion PDF → page unique (haute variable, texte extrait) ──────────────────

/// Fusionne plusieurs PDFs en une seule page très haute (tout le texte en scroll).
pub fn merge_pdfs_single_page(input_paths: &[String], output_path: &str) -> Result<()> {
    use printpdf::BuiltinFont;

    const PAGE_W: f32  = 210.0;
    const MARGIN: f32  = 14.0;
    const FONT_PT: f32 = 7.5;
    const LINE_MM: f32 = 3.5;
    const COLS: usize  = 120;

    let mut combined = String::new();
    for path in input_paths {
        let name = std::path::Path::new(path)
            .file_name().and_then(|n| n.to_str()).unwrap_or(path);
        combined.push_str(&format!("══════ {} ══════\n\n", name));
        match extract_text_from_pdf(path) {
            Ok(t)  => { combined.push_str(&t); combined.push('\n'); }
            Err(_) => combined.push_str("[aucun texte extractible]\n\n"),
        }
    }

    let lines = crate::text_engine::wrap_text(&combined, COLS);
    let page_h = (2.0 * MARGIN + lines.len() as f32 * LINE_MM).max(297.0);

    let (doc, p0, l0) = printpdf::PdfDocument::new(
        "UniversalConverter Merge", Mm(PAGE_W), Mm(page_h), "Layer 1"
    );
    let font = doc.add_builtin_font(BuiltinFont::Courier)
        .map_err(|e| anyhow!("Font: {}", e))?;

    let layer = doc.get_page(p0).get_layer(l0);
    layer.begin_text_section();
    layer.set_font(&font, FONT_PT);
    layer.set_text_cursor(Mm(MARGIN), Mm(page_h - MARGIN));
    layer.set_line_height(LINE_MM * 2.835);
    for line in &lines {
        layer.write_text(line.as_str(), &font);
        layer.add_line_break();
    }
    layer.end_text_section();

    let file = File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    doc.save(&mut BufWriter::new(file))
        .map_err(|e| anyhow!("Sauvegarde: {}", e))?;
    Ok(())
}

// ── Images → PDF ──────────────────────────────────────────────────────────────

pub fn images_to_pdf(image_paths: &[String], output_path: &str) -> Result<()> {
    if image_paths.is_empty() {
        return Err(anyhow!("Aucune image fournie"));
    }

    let (doc, page1, layer1) = PdfDocument::new(
        "UniversalConverter Output",
        Mm(210.0), Mm(297.0), "Layer 1",
    );
    let mut first_page = Some((page1, layer1));

    for img_path in image_paths {
        let img = image::open(img_path)
            .map_err(|e| anyhow!("Ouverture '{}': {}", img_path, e))?;

        let rgb_img = img.to_rgb8();
        let (w, h) = rgb_img.dimensions();
        let dpi = (w as f32 * 25.4) / 210.0;

        let image_obj = ImageXObject {
            width: Px(w as usize),
            height: Px(h as usize),
            color_space: ColorSpace::Rgb,
            bits_per_component: ColorBits::Bit8,
            interpolate: true,
            image_data: rgb_img.into_raw(),
            image_filter: None,
            clipping_bbox: None,
            smask: None,
        };

        let pdf_image = Image::from(image_obj);

        let (page_idx, layer_idx) = if let Some(p) = first_page.take() {
            p
        } else {
            doc.add_page(Mm(210.0), Mm(297.0), "Layer 1")
        };

        let current_layer = doc.get_page(page_idx).get_layer(layer_idx);
        pdf_image.add_to_layer(current_layer, ImageTransform {
            translate_x: Some(Mm(0.0)),
            translate_y: Some(Mm(0.0)),
            dpi: Some(dpi),
            ..Default::default()
        });
    }

    let file = File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    doc.save(&mut BufWriter::new(file))
        .map_err(|e| anyhow!("Sauvegarde PDF: {}", e))?;
    Ok(())
}
