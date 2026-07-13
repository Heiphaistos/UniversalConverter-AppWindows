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
