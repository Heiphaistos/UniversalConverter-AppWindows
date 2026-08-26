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

#[cfg(test)]
mod pdf_write_tests {
    /// Régression printpdf 0.7 -> 0.12 : l'API layer/font a été remplacée par
    /// des `Op` sur `PdfPage`. Vérifie que le PDF écrit est relisible et que
    /// le texte survit à l'aller-retour.
    #[test]
    fn pdf_ecrit_est_relisible() {
        let out = std::env::temp_dir().join("uc_printpdf_test.pdf");
        let out = out.to_str().unwrap();
        let texte = "Ligne un R&D
Ligne deux cafe
Ligne trois";

        crate::text_engine::create_pdf_from_text(texte, out).expect("écriture PDF");

        let meta = std::fs::metadata(out).expect("PDF créé");
        assert!(meta.len() > 500, "PDF suspicieusement petit: {} octets", meta.len());

        let relu = crate::pdf_engine::extract_text_from_pdf(out).expect("relecture PDF");
        let _ = std::fs::remove_file(out);
        for attendu in ["Ligne un", "R&D", "Ligne trois"] {
            assert!(relu.contains(attendu), "texte perdu ({attendu}) dans: {relu}");
        }
    }

    /// Même migration, chemin XObject : `Image::add_to_layer` est devenu
    /// `Op::UseXobject` + `RawImage`. Vérifie que le PDF image est valide.
    #[test]
    fn pdf_image_est_valide() {
        let png = std::env::temp_dir().join("uc_printpdf_test.png");
        let out = std::env::temp_dir().join("uc_printpdf_img.pdf");
        image::RgbImage::from_pixel(32, 16, image::Rgb([200, 30, 30]))
            .save(&png)
            .expect("écriture PNG");

        crate::pdf_engine::images_to_pdf(
            &[png.to_str().unwrap().to_string()],
            out.to_str().unwrap(),
        )
        .expect("écriture PDF image");

        let doc = lopdf::Document::load(&out).expect("PDF image relisible");
        let pages = doc.get_pages().len();
        let _ = std::fs::remove_file(&png);
        let _ = std::fs::remove_file(&out);
        assert_eq!(pages, 1, "une image = une page");
    }
}
