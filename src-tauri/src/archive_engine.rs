/// archive_engine.rs — Conversion d'archives : ZIP ↔ TAR ↔ TAR.GZ, extraction 7Z.
/// Tout se fait en mémoire (pas d'extraction disque) — protection zip-slip par
/// assainissement des noms d'entrées + plafond anti zip-bomb.
/// Code partagé desktop / web.

use anyhow::{anyhow, Result};
use std::io::{Read, Write};

/// Plafond cumulé du contenu décompressé (anti zip-bomb).
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024; // 512 MB
/// Nombre maximal d'entrées.
const MAX_ENTRIES: usize = 10_000;

struct Entry {
    name: String,
    data: Vec<u8>,
}

/// Neutralise les chemins dangereux dans un nom d'entrée (zip-slip).
fn sanitize_entry_name(name: &str) -> Option<String> {
    let normalized = name.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .collect();
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("/");
    // Refuser les chemins absolus Windows résiduels ("C:", etc.)
    if joined.contains(':') {
        return None;
    }
    Some(joined)
}

// ─── Lecture ──────────────────────────────────────────────────────────────────

fn read_zip(input_path: &str) -> Result<Vec<Entry>> {
    let file = std::fs::File::open(input_path)
        .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| anyhow!("ZIP invalide: {}", e))?;
    let mut entries = Vec::new();
    let mut total: u64 = 0;

    for i in 0..archive.len().min(MAX_ENTRIES) {
        let mut entry = archive.by_index(i).map_err(|e| anyhow!("Entrée ZIP {}: {}", i, e))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = sanitize_entry_name(entry.name()) else { continue };
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL_BYTES {
            return Err(anyhow!("Archive trop volumineuse une fois décompressée (> 512 MB)"));
        }
        let mut data = Vec::with_capacity(entry.size().min(MAX_TOTAL_BYTES) as usize);
        entry
            .by_ref()
            .take(MAX_TOTAL_BYTES)
            .read_to_end(&mut data)
            .map_err(|e| anyhow!("Lecture entrée '{}': {}", name, e))?;
        entries.push(Entry { name, data });
    }
    Ok(entries)
}

fn read_tar_from<R: Read>(reader: R) -> Result<Vec<Entry>> {
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    let mut total: u64 = 0;

    for entry in archive.entries().map_err(|e| anyhow!("TAR invalide: {}", e))? {
        let mut entry = entry.map_err(|e| anyhow!("Entrée TAR: {}", e))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let raw_name = entry
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let Some(name) = sanitize_entry_name(&raw_name) else { continue };
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL_BYTES || entries.len() >= MAX_ENTRIES {
            return Err(anyhow!("Archive trop volumineuse une fois décompressée (> 512 MB)"));
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_TOTAL_BYTES)
            .read_to_end(&mut data)
            .map_err(|e| anyhow!("Lecture entrée '{}': {}", name, e))?;
        entries.push(Entry { name, data });
    }
    Ok(entries)
}

fn read_7z(input_path: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut total: u64 = 0;
    let mut reader = sevenz_rust::SevenZReader::open(input_path, sevenz_rust::Password::empty())
        .map_err(|e| anyhow!("7Z invalide: {}", e))?;

    reader
        .for_each_entries(|entry, rd| {
            if entry.is_directory() {
                return Ok(true);
            }
            let Some(name) = sanitize_entry_name(entry.name()) else {
                return Ok(true);
            };
            total = total.saturating_add(entry.size());
            if total > MAX_TOTAL_BYTES || entries.len() >= MAX_ENTRIES {
                return Err(sevenz_rust::Error::other("Archive décompressée > 512 MB"));
            }
            let mut data = Vec::new();
            rd.take(MAX_TOTAL_BYTES)
                .read_to_end(&mut data)
                .map_err(sevenz_rust::Error::io)?;
            entries.push(Entry { name, data });
            Ok(true)
        })
        .map_err(|e| anyhow!("Extraction 7Z: {}", e))?;
    Ok(entries)
}

fn read_archive(input_path: &str, in_ext: &str) -> Result<Vec<Entry>> {
    match in_ext {
        "zip" => read_zip(input_path),
        "tar" => {
            let file = std::fs::File::open(input_path)
                .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;
            read_tar_from(std::io::BufReader::new(file))
        }
        "tgz" | "gz" => {
            let file = std::fs::File::open(input_path)
                .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;
            let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
            read_tar_from(gz)
        }
        "7z" => read_7z(input_path),
        _ => Err(anyhow!("Format d'archive non supporté: {}", in_ext)),
    }
}

// ─── Écriture ─────────────────────────────────────────────────────────────────

fn write_zip(entries: &[Entry], output_path: &str) -> Result<()> {
    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    for entry in entries {
        writer
            .start_file(&entry.name, options)
            .map_err(|e| anyhow!("ZIP start_file '{}': {}", entry.name, e))?;
        writer
            .write_all(&entry.data)
            .map_err(|e| anyhow!("ZIP write '{}': {}", entry.name, e))?;
    }
    writer.finish().map_err(|e| anyhow!("ZIP finish: {}", e))?;
    Ok(())
}

fn append_tar_entries<W: Write>(builder: &mut tar::Builder<W>, entries: &[Entry]) -> Result<()> {
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(entry.data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, &entry.name, entry.data.as_slice())
            .map_err(|e| anyhow!("TAR append '{}': {}", entry.name, e))?;
    }
    Ok(())
}

fn write_tar(entries: &[Entry], output_path: &str) -> Result<()> {
    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let mut builder = tar::Builder::new(std::io::BufWriter::new(file));
    append_tar_entries(&mut builder, entries)?;
    builder.finish().map_err(|e| anyhow!("TAR finish: {}", e))?;
    Ok(())
}

fn write_tgz(entries: &[Entry], output_path: &str) -> Result<()> {
    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let gz = flate2::write::GzEncoder::new(
        std::io::BufWriter::new(file),
        flate2::Compression::default(),
    );
    let mut builder = tar::Builder::new(gz);
    append_tar_entries(&mut builder, entries)?;
    let gz = builder.into_inner().map_err(|e| anyhow!("TAR finish: {}", e))?;
    gz.finish().map_err(|e| anyhow!("GZ finish: {}", e))?;
    Ok(())
}

// ─── Conversion ───────────────────────────────────────────────────────────────

pub fn convert_archive(input_path: &str, in_ext: &str, output_path: &str, fmt: &str) -> Result<()> {
    let entries = read_archive(input_path, in_ext)?;
    if entries.is_empty() {
        return Err(anyhow!("Archive vide ou entrées toutes invalides"));
    }
    match fmt {
        "zip" => write_zip(&entries, output_path),
        "tar" => write_tar(&entries, output_path),
        "tgz" => write_tgz(&entries, output_path),
        _ => Err(anyhow!("Format d'archive de sortie non supporté: {}", fmt)),
    }
}
