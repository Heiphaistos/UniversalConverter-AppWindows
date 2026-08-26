mod archive_engine;
mod audio_engine;
mod commands;
mod conversion_engine;
mod data_engine;
mod doc_engine;
mod office_engine;
mod pdf_engine;
mod text_engine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::convert_file,
            commands::get_formats_for_extension,
            commands::merge_images_to_pdf,
            commands::get_thumbnail,
            commands::get_file_size,
            commands::get_pdf_page_count,
            commands::split_pdf_command,
            commands::merge_pdfs_command,
            commands::merge_pdfs_mode_command,
            commands::zip_files_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Décode et déséchappe le contenu d'un `BytesText` quick-xml.
///
/// Remplace `BytesText::unescape()`, retiré en quick-xml 0.38 : `xml10_content()`
/// ne fait que décoder et normaliser les fins de ligne, il faut résoudre les
/// entités (`&amp;`, `&#233;`…) dans un second temps.
pub fn xml_text(t: &quick_xml::events::BytesText<'_>) -> String {
    match t.xml10_content() {
        Ok(raw) => quick_xml::escape::unescape(&raw)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| raw.into_owned()),
        Err(_) => String::new(),
    }
}

/// Résout une référence d'entité quick-xml (`Event::GeneralRef`).
///
/// Depuis quick-xml 0.38 le lecteur coupe le texte à chaque `&...;` et émet
/// l'entité comme événement distinct : sans ce traitement, tout texte extrait
/// serait tronqué au premier `&amp;` ou `&#233;`.
pub fn xml_entity(e: &quick_xml::events::BytesRef<'_>) -> String {
    if let Ok(Some(c)) = e.resolve_char_ref() {
        return c.to_string();
    }
    let name = e.decode().unwrap_or_default();
    let raw = format!("&{};", name);
    quick_xml::escape::unescape(&raw)
        .map(|s| s.into_owned())
        .unwrap_or(raw)
}

#[cfg(test)]
mod xml_text_tests {
    /// Régression quick-xml 0.36 -> 0.41 : le lecteur coupe le texte à chaque
    /// entité et l'émet en `Event::GeneralRef`. Sans l'arm dédiée, tout texte
    /// extrait d'un XML/DOCX/XLSX était tronqué au premier `&amp;`.
    #[test]
    fn lecture_xml_resout_les_entites() {
        let path = std::env::temp_dir().join("uc_entites_test.xml");
        std::fs::write(&path, "<r><a>R&amp;D</a><b>caf&#233; &lt;x&gt;</b></r>")
            .expect("écriture fichier de test");
        let v = crate::data_engine::read_value(path.to_str().unwrap(), "xml")
            .expect("lecture xml");
        let dump = v.to_string();
        let _ = std::fs::remove_file(&path);
        assert!(dump.contains("R&D"), "entité nommée perdue: {dump}");
        assert!(dump.contains("café"), "entité numérique perdue: {dump}");
        assert!(dump.contains("<x>"), "entités lt/gt perdues: {dump}");
    }
}
