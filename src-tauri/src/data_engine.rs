/// data_engine.rs — Conversions de données structurées et texte balisé.
/// Pivot : serde_json::Value. JSON ↔ YAML ↔ TOML ↔ XML ↔ CSV, SRT ↔ VTT, RTF → TXT.
/// Code partagé desktop / web (aucune dépendance Tauri ni axum).

use anyhow::{anyhow, Result};
use serde_json::Value;

// ─── Lecture : format → Value ─────────────────────────────────────────────────

pub fn read_value(input_path: &str, ext: &str) -> Result<Value> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    match ext {
        "json" => serde_json::from_str(&raw).map_err(|e| anyhow!("JSON invalide: {}", e)),
        "yaml" | "yml" => serde_yaml::from_str(&raw).map_err(|e| anyhow!("YAML invalide: {}", e)),
        "toml" => {
            let t: toml::Value = toml::from_str(&raw).map_err(|e| anyhow!("TOML invalide: {}", e))?;
            serde_json::to_value(t).map_err(|e| anyhow!("TOML → JSON: {}", e))
        }
        "xml" => xml_to_value(&raw),
        "csv" => csv_to_value(&raw),
        _ => Err(anyhow!("Format d'entrée data non supporté: {}", ext)),
    }
}

// ─── Écriture : Value → format ────────────────────────────────────────────────

pub fn write_value(value: &Value, output_path: &str, fmt: &str) -> Result<()> {
    let out = match fmt {
        "json" => serde_json::to_string_pretty(value)?,
        "yaml" | "yml" => serde_yaml::to_string(value)?,
        "toml" => {
            // TOML exige une table au sommet : envelopper les tableaux/scalaires.
            let wrapped = match value {
                Value::Object(_) => value.clone(),
                other => serde_json::json!({ "items": other }),
            };
            let t: toml::Value = serde_json::from_value(wrapped)
                .map_err(|e| anyhow!("Structure non représentable en TOML: {}", e))?;
            toml::to_string_pretty(&t).map_err(|e| anyhow!("TOML encode: {}", e))?
        }
        "xml" => value_to_xml(value),
        "csv" => value_to_csv(value)?,
        "txt" => serde_json::to_string_pretty(value)?,
        _ => return Err(anyhow!("Format de sortie data non supporté: {}", fmt)),
    };
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

/// Conversion data → data en un appel (utilisée par dispatch/commands).
pub fn convert_data(input_path: &str, in_ext: &str, output_path: &str, fmt: &str) -> Result<()> {
    let value = read_value(input_path, in_ext)?;
    write_value(&value, output_path, fmt)
}

// ─── XML → Value ──────────────────────────────────────────────────────────────

fn xml_to_value(raw: &str) -> Result<Value> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);

    // Pile d'objets en construction : (nom, map)
    let mut stack: Vec<(String, serde_json::Map<String, Value>)> = Vec::new();
    let mut root: Option<Value> = None;
    let mut root_name = String::new();

    fn insert_child(map: &mut serde_json::Map<String, Value>, name: &str, child: Value) {
        match map.get_mut(name) {
            Some(Value::Array(arr)) => arr.push(child),
            Some(existing) => {
                let prev = existing.take();
                *existing = Value::Array(vec![prev, child]);
            }
            None => { map.insert(name.to_string(), child); }
        }
    }

    fn finalize(map: serde_json::Map<String, Value>) -> Value {
        // Élément ne contenant que du texte → chaîne simple
        if map.len() == 1 {
            if let Some(Value::String(s)) = map.get("#text") {
                return Value::String(s.clone());
            }
        }
        if map.is_empty() { return Value::Null; }
        Value::Object(map)
    }

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut map = serde_json::Map::new();
                for attr in e.attributes().flatten() {
                    let k = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
                    let v = String::from_utf8_lossy(&attr.value).to_string();
                    map.insert(k, Value::String(v));
                }
                stack.push((name, map));
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut map = serde_json::Map::new();
                for attr in e.attributes().flatten() {
                    let k = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
                    let v = String::from_utf8_lossy(&attr.value).to_string();
                    map.insert(k, Value::String(v));
                }
                let child = finalize(map);
                if let Some((_, parent)) = stack.last_mut() {
                    insert_child(parent, &name, child);
                } else {
                    root_name = name;
                    root = Some(child);
                }
            }
            Ok(Event::Text(t)) => {
                let text = t.unescape().unwrap_or_default().to_string();
                if !text.is_empty() {
                    if let Some((_, map)) = stack.last_mut() {
                        match map.get_mut("#text") {
                            Some(Value::String(existing)) => existing.push_str(&text),
                            _ => { map.insert("#text".into(), Value::String(text)); }
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                if let Some((name, map)) = stack.pop() {
                    let child = finalize(map);
                    if let Some((_, parent)) = stack.last_mut() {
                        insert_child(parent, &name, child);
                    } else {
                        root_name = name;
                        root = Some(child);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {} // commentaires, CDATA, PI, DOCTYPE ignorés
            Err(e) => return Err(anyhow!("XML invalide: {}", e)),
        }
    }

    match root {
        Some(v) => Ok(serde_json::json!({ root_name: v })),
        None => Err(anyhow!("XML vide ou sans élément racine")),
    }
}

// ─── Value → XML ──────────────────────────────────────────────────────────────

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Nom d'élément XML valide (fallback "item").
fn xml_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() || cleaned.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("item{}", if cleaned.is_empty() { String::new() } else { format!("_{cleaned}") })
    } else {
        cleaned
    }
}

fn value_to_xml(value: &Value) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    // Objet racine à clé unique → cette clé devient l'élément racine.
    match value {
        Value::Object(map) if map.len() == 1 => {
            let (k, v) = map.iter().next().expect("map non vide");
            write_xml_node(&mut out, &xml_name(k), v, 0);
        }
        other => write_xml_node(&mut out, "root", other, 0),
    }
    out
}

fn write_xml_node(out: &mut String, name: &str, value: &Value, depth: usize) {
    let pad = "  ".repeat(depth);
    match value {
        Value::Array(arr) => {
            for item in arr {
                write_xml_node(out, name, item, depth);
            }
        }
        Value::Object(map) => {
            out.push_str(&format!("{pad}<{name}"));
            // Attributs (@clé) d'abord
            for (k, v) in map.iter().filter(|(k, _)| k.starts_with('@')) {
                if let Value::String(s) = v {
                    out.push_str(&format!(" {}=\"{}\"", xml_name(&k[1..]), xml_escape(s)));
                }
            }
            let children: Vec<(&String, &Value)> =
                map.iter().filter(|(k, _)| !k.starts_with('@')).collect();
            if children.is_empty() {
                out.push_str("/>\n");
                return;
            }
            // Contenu texte pur
            if children.len() == 1 && children[0].0 == "#text" {
                if let Value::String(s) = children[0].1 {
                    out.push_str(&format!(">{}</{}>\n", xml_escape(s), name));
                    return;
                }
            }
            out.push_str(">\n");
            for (k, v) in children {
                if k == "#text" {
                    if let Value::String(s) = v {
                        out.push_str(&format!("{pad}  {}\n", xml_escape(s)));
                    }
                } else {
                    write_xml_node(out, &xml_name(k), v, depth + 1);
                }
            }
            out.push_str(&format!("{pad}</{name}>\n"));
        }
        Value::Null => out.push_str(&format!("{pad}<{name}/>\n")),
        scalar => {
            let text = match scalar {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out.push_str(&format!("{pad}<{name}>{}</{name}>\n", xml_escape(&text)));
        }
    }
}

// ─── CSV ↔ Value ──────────────────────────────────────────────────────────────

fn csv_to_value(raw: &str) -> Result<Value> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(raw.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| anyhow!("CSV invalide: {}", e))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| anyhow!("CSV invalide: {}", e))?;
        let mut obj = serde_json::Map::new();
        for (i, field) in record.iter().enumerate() {
            let key = headers
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("col{}", i + 1));
            obj.insert(key, Value::String(field.to_string()));
        }
        rows.push(Value::Object(obj));
    }
    Ok(Value::Array(rows))
}

fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn value_to_csv(value: &Value) -> Result<String> {
    let records = value
        .as_array()
        .ok_or_else(|| anyhow!("CSV attend un tableau d'objets au sommet"))?;
    let mut writer = csv::Writer::from_writer(Vec::new());

    // Union ordonnée des clés de tous les objets
    let mut headers: Vec<String> = Vec::new();
    for r in records {
        if let Some(obj) = r.as_object() {
            for k in obj.keys() {
                if !headers.contains(k) {
                    headers.push(k.clone());
                }
            }
        }
    }
    if headers.is_empty() {
        return Ok(String::new());
    }
    writer.write_record(&headers)?;
    for r in records {
        if let Some(obj) = r.as_object() {
            let row: Vec<String> = headers
                .iter()
                .map(|h| obj.get(h).map(scalar_to_string).unwrap_or_default())
                .collect();
            writer.write_record(&row)?;
        }
    }
    let bytes = writer.into_inner().map_err(|e| anyhow!("CSV encode: {}", e))?;
    String::from_utf8(bytes).map_err(|e| anyhow!("CSV UTF-8: {}", e))
}

// ─── CSV → tableau Markdown / HTML ────────────────────────────────────────────

pub fn csv_to_markdown(input_path: &str, output_path: &str) -> Result<()> {
    let rows = read_csv_rows(input_path)?;
    if rows.is_empty() {
        std::fs::write(output_path, "")?;
        return Ok(());
    }
    let mut out = String::new();
    let escape = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    out.push_str(&format!("| {} |\n", rows[0].iter().map(|c| escape(c)).collect::<Vec<_>>().join(" | ")));
    out.push_str(&format!("|{}\n", " --- |".repeat(rows[0].len())));
    for row in &rows[1..] {
        out.push_str(&format!("| {} |\n", row.iter().map(|c| escape(c)).collect::<Vec<_>>().join(" | ")));
    }
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

pub fn csv_to_html_table(input_path: &str, output_path: &str) -> Result<()> {
    let rows = read_csv_rows(input_path)?;
    let mut out = String::from(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><style>table{border-collapse:collapse}td,th{border:1px solid #999;padding:4px 8px}</style></head><body>\n<table>\n",
    );
    for (i, row) in rows.iter().enumerate() {
        let tag = if i == 0 { "th" } else { "td" };
        out.push_str("<tr>");
        for cell in row {
            out.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(cell)));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n</body></html>\n");
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

fn read_csv_rows(input_path: &str) -> Result<Vec<Vec<String>>> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_reader(raw.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| anyhow!("CSV invalide: {}", e))?;
        rows.push(record.iter().map(|f| f.to_string()).collect());
    }
    Ok(rows)
}

// ─── Sous-titres SRT ↔ VTT ────────────────────────────────────────────────────

fn is_timestamp_line(line: &str) -> bool {
    line.contains("-->")
}

pub fn srt_to_vtt(input_path: &str, output_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let mut out = String::from("WEBVTT\n\n");
    for line in raw.lines() {
        if is_timestamp_line(line) {
            out.push_str(&line.replace(',', "."));
        } else if line.trim().chars().all(|c| c.is_ascii_digit()) && !line.trim().is_empty() {
            continue; // compteur SRT : VTT n'en a pas besoin
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

pub fn vtt_to_srt(input_path: &str, output_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let mut out = String::new();
    let mut counter = 0u32;
    let mut in_note = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("WEBVTT") || trimmed.starts_with("STYLE") || trimmed.starts_with("REGION") {
            in_note = true; // bloc d'en-tête : sauté jusqu'à la ligne vide
            continue;
        }
        if trimmed.starts_with("NOTE") {
            in_note = true;
            continue;
        }
        if trimmed.is_empty() {
            if !in_note && counter > 0 {
                out.push('\n');
            }
            in_note = false;
            continue;
        }
        if in_note {
            continue;
        }
        if is_timestamp_line(trimmed) {
            counter += 1;
            out.push_str(&format!("{counter}\n"));
            // Timestamps "." → "," ; réglages de cue (align:, position:…) supprimés
            let ts = trimmed.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            // Format VTT court "MM:SS.mmm" → SRT exige "HH:MM:SS,mmm"
            let normalized = ts
                .split(" --> ")
                .map(|t| {
                    let t = t.replace('.', ",");
                    if t.matches(':').count() == 1 { format!("00:{t}") } else { t }
                })
                .collect::<Vec<_>>()
                .join(" --> ");
            out.push_str(&normalized);
            out.push('\n');
        } else {
            // Identifiant de cue VTT (ligne précédant le timestamp) : ignoré,
            // sauf si c'est du texte de sous-titre (après un timestamp déjà écrit)
            if counter > 0 {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

/// SRT ou VTT → texte brut (dialogue seul, sans timestamps ni balises).
pub fn subtitles_to_text(input_path: &str, output_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let mut out = String::new();
    let mut in_header = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("WEBVTT") || trimmed.starts_with("NOTE")
            || trimmed.starts_with("STYLE") || trimmed.starts_with("REGION") {
            in_header = true;
            continue;
        }
        if trimmed.is_empty() { in_header = false; continue; }
        if in_header || is_timestamp_line(trimmed) { continue; }
        if trimmed.chars().all(|c| c.is_ascii_digit()) { continue; }
        // Balises <i>, <b>, {\an8}… supprimées
        let mut clean = String::new();
        let mut in_tag = false;
        for c in trimmed.chars() {
            match c {
                '<' | '{' => in_tag = true,
                '>' | '}' => in_tag = false,
                _ if !in_tag => clean.push(c),
                _ => {}
            }
        }
        if !clean.trim().is_empty() {
            out.push_str(clean.trim());
            out.push('\n');
        }
    }
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

// ─── RTF → TXT ────────────────────────────────────────────────────────────────

/// Extracteur de texte RTF minimaliste : gère \par, \tab, \'xx, \uN,
/// et saute les groupes de destination (fonttbl, colortbl, pict, info…).
pub fn rtf_to_text(input_path: &str, output_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    if !raw.trim_start().starts_with("{\\rtf") {
        return Err(anyhow!("Fichier RTF invalide (en-tête absent)"));
    }

    const SKIP_DESTINATIONS: &[&str] = &[
        "fonttbl", "colortbl", "stylesheet", "info", "pict", "object",
        "header", "footer", "themedata", "colorschememapping", "datastore",
        "latentstyles", "listtable", "listoverridetable", "generator",
    ];

    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    // Profondeur d'un groupe à sauter (None = pas de saut en cours)
    let mut skip_until_depth: Option<usize> = None;
    let mut depth = 0usize;
    let mut pending_unicode_skip = 0usize; // fallback après \uN (1 char par défaut)

    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '{' => { depth += 1; i += 1; }
            '}' => {
                depth = depth.saturating_sub(1);
                if let Some(d) = skip_until_depth {
                    if depth < d { skip_until_depth = None; }
                }
                i += 1;
            }
            '\\' => {
                i += 1;
                if i >= bytes.len() { break; }
                let next = bytes[i] as char;
                if next == '\'' {
                    // \'xx : octet hexadécimal (approx. latin-1)
                    if i + 2 < bytes.len() {
                        let hex = &raw[i + 1..i + 3];
                        if let Ok(b) = u8::from_str_radix(hex, 16) {
                            if skip_until_depth.is_none() {
                                if pending_unicode_skip > 0 { pending_unicode_skip -= 1; }
                                else { out.push(b as char); }
                            }
                        }
                        i += 3;
                    } else { i += 1; }
                } else if next == '\\' || next == '{' || next == '}' {
                    if skip_until_depth.is_none() { out.push(next); }
                    i += 1;
                } else if next == '*' {
                    // \* : destination ignorable → sauter le groupe courant
                    if skip_until_depth.is_none() { skip_until_depth = Some(depth); }
                    i += 1;
                } else if next.is_ascii_alphabetic() {
                    // Mot de contrôle
                    let start = i;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() { i += 1; }
                    let word = &raw[start..i];
                    // Paramètre numérique optionnel
                    let num_start = i;
                    if i < bytes.len() && (bytes[i] as char == '-') { i += 1; }
                    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() { i += 1; }
                    let param: Option<i64> = raw[num_start..i].parse().ok();
                    // Espace délimiteur consommé
                    if i < bytes.len() && bytes[i] as char == ' ' { i += 1; }

                    if SKIP_DESTINATIONS.contains(&word) && skip_until_depth.is_none() {
                        skip_until_depth = Some(depth);
                    } else if skip_until_depth.is_none() {
                        match word {
                            "par" | "line" | "sect" | "page" => out.push('\n'),
                            "tab" => out.push('\t'),
                            "emdash" => out.push('—'),
                            "endash" => out.push('–'),
                            "lquote" => out.push('\u{2018}'),
                            "rquote" => out.push('\u{2019}'),
                            "ldblquote" => out.push('\u{201C}'),
                            "rdblquote" => out.push('\u{201D}'),
                            "u" => {
                                if let Some(n) = param {
                                    let code = if n < 0 { (n + 65536) as u32 } else { n as u32 };
                                    if let Some(ch) = char::from_u32(code) { out.push(ch); }
                                    pending_unicode_skip = 1;
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    i += 1; // symbole de contrôle inconnu
                }
            }
            '\r' | '\n' => { i += 1; }
            _ => {
                if skip_until_depth.is_none() {
                    if pending_unicode_skip > 0 { pending_unicode_skip -= 1; }
                    else { out.push(c); }
                }
                i += 1;
            }
        }
    }

    std::fs::write(output_path, out.trim()).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}
