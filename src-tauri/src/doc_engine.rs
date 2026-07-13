/// doc_engine.rs — Documents : EPUB (lecture/écriture), DOCX (écriture),
/// HTML → Markdown, TXT → HTML. Code partagé desktop / web.

use anyhow::{anyhow, Result};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn md_string_to_html(md: &str) -> String {
    let parser = pulldown_cmark::Parser::new_ext(md, pulldown_cmark::Options::all());
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string())
}

// ─── HTML → Markdown ──────────────────────────────────────────────────────────

pub fn html_to_md(input_path: &str, output_path: &str) -> Result<()> {
    let html = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let md = htmd::convert(&html).map_err(|e| anyhow!("HTML → Markdown: {}", e))?;
    std::fs::write(output_path, md).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

// ─── TXT → HTML ───────────────────────────────────────────────────────────────

pub fn txt_to_html(input_path: &str, output_path: &str) -> Result<()> {
    let text = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let html = text_to_html_string(&file_stem(input_path), &text);
    std::fs::write(output_path, html).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

fn text_to_html_string(title: &str, text: &str) -> String {
    let body: String = text
        .split("\n\n")
        .map(|block| format!("<p>{}</p>\n", html_escape(block.trim()).replace('\n', "<br>\n")))
        .collect();
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{}</body></html>\n",
        html_escape(title),
        body
    )
}

// ─── EPUB → HTML / TXT ────────────────────────────────────────────────────────

/// Concatène le contenu XHTML de tous les chapitres du spine.
fn epub_chapters(input_path: &str) -> Result<Vec<String>> {
    let mut doc = epub::doc::EpubDoc::new(input_path)
        .map_err(|e| anyhow!("EPUB invalide '{}': {}", input_path, e))?;
    let mut chapters = Vec::new();
    loop {
        if let Some((content, _mime)) = doc.get_current_str() {
            chapters.push(content);
        }
        if !doc.go_next() {
            break;
        }
    }
    if chapters.is_empty() {
        return Err(anyhow!("EPUB sans contenu lisible"));
    }
    Ok(chapters)
}

/// Extrait le corps (<body>…</body>) d'un document XHTML, ou le document entier.
fn extract_body(xhtml: &str) -> &str {
    // ASCII lowercase : longueurs d'octets préservées (indices partagés avec xhtml)
    let lower: String = xhtml.chars().map(|c| c.to_ascii_lowercase()).collect();
    let start = lower.find("<body");
    let end = lower.rfind("</body>");
    match (start, end) {
        (Some(s), Some(e)) if e > s => {
            let after_tag = xhtml[s..].find('>').map(|i| s + i + 1).unwrap_or(s);
            &xhtml[after_tag..e]
        }
        _ => xhtml,
    }
}

pub fn epub_to_html(input_path: &str, output_path: &str) -> Result<()> {
    let chapters = epub_chapters(input_path)?;
    let title = file_stem(input_path);
    let mut html = format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n",
        html_escape(&title)
    );
    for c in &chapters {
        html.push_str(extract_body(c));
        html.push_str("\n<hr>\n");
    }
    html.push_str("</body></html>\n");
    std::fs::write(output_path, html).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

/// EPUB → texte brut (balises supprimées, scripts/styles ignorés).
pub fn epub_to_text(input_path: &str, output_path: &str) -> Result<()> {
    let text = epub_text_string(input_path)?;
    std::fs::write(output_path, text)
        .map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

fn epub_text_string(input_path: &str) -> Result<String> {
    let chapters = epub_chapters(input_path)?;
    let mut out = String::new();
    for c in &chapters {
        out.push_str(&strip_html_tags(extract_body(c)));
        out.push_str("\n\n");
    }
    Ok(out.trim().to_string())
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::new();
    // ASCII lowercase uniquement : préserve les longueurs d'octets (indices partagés)
    let lower: String = html.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut i = 0usize;
    let bytes_len = html.len();

    while i < bytes_len {
        if lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
            let close = if lower[i..].starts_with("<script") { "</script>" } else { "</style>" };
            match lower[i..].find(close) {
                Some(rel) => { i += rel + close.len(); continue; }
                None => break,
            }
        }
        if html[i..].starts_with('<') {
            match html[i..].find('>') {
                Some(rel) => {
                    let tag = &lower[i..i + rel + 1];
                    if tag.starts_with("<p") || tag.starts_with("</p")
                        || tag.starts_with("<br") || tag.starts_with("<div")
                        || tag.starts_with("</h") || tag.starts_with("<li") {
                        out.push('\n');
                    }
                    i += rel + 1;
                    continue;
                }
                None => break,
            }
        }
        let ch_end = html[i..]
            .char_indices()
            .nth(1)
            .map(|(o, _)| i + o)
            .unwrap_or(bytes_len);
        out.push_str(&html[i..ch_end]);
        i = ch_end;
    }
    // Entités courantes
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    // Compression des lignes vides multiples
    let mut result = String::new();
    let mut blank = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank <= 1 { result.push('\n'); }
        } else {
            blank = 0;
            result.push_str(line.trim());
            result.push('\n');
        }
    }
    result
}

// ─── TXT / MD / HTML → EPUB ───────────────────────────────────────────────────

fn build_epub(title: &str, body_html: &str, output_path: &str) -> Result<()> {
    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    let xhtml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">\n<head><title>{}</title></head>\n<body>\n{}\n</body>\n</html>",
        html_escape(title),
        body_html
    );

    let zip = ZipLibrary::new().map_err(|e| anyhow!("EPUB zip: {}", e))?;
    let mut builder = EpubBuilder::new(zip).map_err(|e| anyhow!("EPUB builder: {}", e))?;
    builder
        .metadata("title", title)
        .map_err(|e| anyhow!("EPUB metadata: {}", e))?;
    builder
        .metadata("author", "UniversalConverter")
        .map_err(|e| anyhow!("EPUB metadata: {}", e))?;
    builder
        .add_content(
            EpubContent::new("content.xhtml", xhtml.as_bytes())
                .title(title)
                .reftype(epub_builder::ReferenceType::Text),
        )
        .map_err(|e| anyhow!("EPUB contenu: {}", e))?;

    let mut out: Vec<u8> = Vec::new();
    builder
        .generate(&mut out)
        .map_err(|e| anyhow!("EPUB génération: {}", e))?;
    std::fs::write(output_path, out).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

pub fn txt_to_epub(input_path: &str, output_path: &str) -> Result<()> {
    let text = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let body: String = text
        .split("\n\n")
        .map(|b| format!("<p>{}</p>\n", html_escape(b.trim()).replace('\n', "<br/>\n")))
        .collect();
    build_epub(&file_stem(input_path), &body, output_path)
}

pub fn md_to_epub(input_path: &str, output_path: &str) -> Result<()> {
    let md = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let html = md_string_to_html(&md);
    // pulldown-cmark émet du HTML avec balises auto-fermantes valides XHTML (<br />)
    build_epub(&file_stem(input_path), &html, output_path)
}

pub fn html_to_epub(input_path: &str, output_path: &str) -> Result<()> {
    // HTML arbitraire = XHTML non garanti → repasser par Markdown puis re-générer
    // du XHTML propre (perte de style acceptée, structure préservée).
    let html = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    let md = htmd::convert(&html).map_err(|e| anyhow!("HTML → Markdown: {}", e))?;
    build_epub(&file_stem(input_path), &md_string_to_html(&md), output_path)
}

// ─── TXT / MD → DOCX ──────────────────────────────────────────────────────────

fn docx_paragraph(text: &str, size: Option<usize>, bold: bool) -> docx_rs::Paragraph {
    use docx_rs::{Paragraph, Run};
    let mut run = Run::new().add_text(text);
    if let Some(s) = size {
        run = run.size(s);
    }
    if bold {
        run = run.bold();
    }
    Paragraph::new().add_run(run)
}

fn write_docx(paragraphs: Vec<docx_rs::Paragraph>, output_path: &str) -> Result<()> {
    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let mut docx = docx_rs::Docx::new();
    for p in paragraphs {
        docx = docx.add_paragraph(p);
    }
    docx.build()
        .pack(file)
        .map_err(|e| anyhow!("DOCX pack: {}", e))?;
    Ok(())
}

pub fn txt_to_docx(input_path: &str, output_path: &str) -> Result<()> {
    let text = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    text_to_docx(&text, output_path)
}

fn text_to_docx(text: &str, output_path: &str) -> Result<()> {
    let paragraphs = text
        .lines()
        .map(|line| docx_paragraph(line, None, false))
        .collect();
    write_docx(paragraphs, output_path)
}

/// Markdown → DOCX ligne à ligne : titres stylés, listes préfixées "•".
pub fn md_to_docx(input_path: &str, output_path: &str) -> Result<()> {
    let md = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let strip_inline = |s: &str| s.replace("**", "").replace('`', "");
    let mut paragraphs = Vec::new();

    for line in md.lines() {
        let trimmed = line.trim_end();
        let p = if let Some(t) = trimmed.strip_prefix("### ") {
            docx_paragraph(&strip_inline(t), Some(28), true)
        } else if let Some(t) = trimmed.strip_prefix("## ") {
            docx_paragraph(&strip_inline(t), Some(32), true)
        } else if let Some(t) = trimmed.strip_prefix("# ") {
            docx_paragraph(&strip_inline(t), Some(36), true)
        } else if let Some(t) = trimmed.trim_start().strip_prefix("- ")
            .or_else(|| trimmed.trim_start().strip_prefix("* ")) {
            docx_paragraph(&format!("• {}", strip_inline(t)), None, false)
        } else {
            docx_paragraph(&strip_inline(trimmed), None, false)
        };
        paragraphs.push(p);
    }
    write_docx(paragraphs, output_path)
}

// ─── Écriture RTF ─────────────────────────────────────────────────────────────

fn rtf_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\par\n"),
            '\r' => {}
            '\t' => out.push_str("\\tab "),
            c if (c as u32) < 128 => out.push(c),
            c => {
                // Unicode : \uN suivi d'un caractère de repli '?' ; N signé 16 bits
                let code = c as u32;
                if code <= 32767 {
                    out.push_str(&format!("\\u{}?", code));
                } else if code <= 65535 {
                    out.push_str(&format!("\\u{}?", code as i32 - 65536));
                } else {
                    // Hors BMP : paire de substitution
                    let mut buf = [0u16; 2];
                    for unit in c.encode_utf16(&mut buf) {
                        let v = *unit as i32;
                        out.push_str(&format!("\\u{}?", if v > 32767 { v - 65536 } else { v }));
                    }
                }
            }
        }
    }
    out
}

/// Texte brut → document RTF (police Calibri 11pt, un \par par ligne).
pub fn write_rtf(text: &str, output_path: &str) -> Result<()> {
    let rtf = format!(
        "{{\\rtf1\\ansi\\deff0{{\\fonttbl{{\\f0 Calibri;}}}}\\f0\\fs22\n{}\n}}",
        rtf_escape(text)
    );
    std::fs::write(output_path, rtf).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

// ─── Écriture ODT (OpenDocument Text) ─────────────────────────────────────────

fn xml_escape_odt(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Texte brut → document ODT minimal (mimetype STORED en 1re entrée, exigence ODF).
pub fn write_odt(text: &str, output_path: &str) -> Result<()> {
    use std::io::Write;
    use zip::write::FileOptions;

    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let mut z = zip::ZipWriter::new(file);

    let stored: FileOptions<()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated: FileOptions<()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    z.start_file("mimetype", stored).map_err(|e| anyhow!("ODT mimetype: {}", e))?;
    z.write_all(b"application/vnd.oasis.opendocument.text")
        .map_err(|e| anyhow!("ODT mimetype: {}", e))?;

    let mut content = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         office:version=\"1.2\"><office:body><office:text>",
    );
    for line in text.lines() {
        content.push_str(&format!("<text:p>{}</text:p>", xml_escape_odt(line)));
    }
    content.push_str("</office:text></office:body></office:document-content>");

    z.start_file("content.xml", deflated).map_err(|e| anyhow!("ODT content: {}", e))?;
    z.write_all(content.as_bytes()).map_err(|e| anyhow!("ODT content: {}", e))?;

    z.start_file("styles.xml", deflated).map_err(|e| anyhow!("ODT styles: {}", e))?;
    z.write_all(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles \
          xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" office:version=\"1.2\"/>",
    )
    .map_err(|e| anyhow!("ODT styles: {}", e))?;

    z.start_file("META-INF/manifest.xml", deflated).map_err(|e| anyhow!("ODT manifest: {}", e))?;
    z.write_all(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest \
          xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
          <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\
          <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
          <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
          </manifest:manifest>",
    )
    .map_err(|e| anyhow!("ODT manifest: {}", e))?;

    z.finish().map_err(|e| anyhow!("ODT finish: {}", e))?;
    Ok(())
}

// ─── Lecture ODT / ODP (OpenDocument) ─────────────────────────────────────────

/// Extrait le texte de content.xml d'un fichier OpenDocument (ODT, ODP).
/// Chaque <text:p> ou <text:h> devient une ligne.
pub fn odf_to_text(input_path: &str) -> Result<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use std::io::Read;

    let file = std::fs::File::open(input_path)
        .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| anyhow!("OpenDocument invalide: {}", e))?;
    let mut content = String::new();
    archive
        .by_name("content.xml")
        .map_err(|_| anyhow!("content.xml absent — fichier OpenDocument invalide"))?
        .read_to_string(&mut content)
        .map_err(|e| anyhow!("Lecture content.xml: {}", e))?;

    let mut reader = Reader::from_str(&content);
    let mut out = String::new();
    let mut depth_p = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"text:p" || local == b"text:h" {
                    depth_p += 1;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = name.as_ref();
                if depth_p > 0 && (local == b"text:tab" || local == b"text:line-break") {
                    out.push(if local == b"text:tab" { '\t' } else { '\n' });
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                if name.as_ref() == b"text:p" || name.as_ref() == b"text:h" {
                    depth_p = depth_p.saturating_sub(1);
                    out.push('\n');
                }
            }
            Ok(Event::Text(t)) => {
                if depth_p > 0 {
                    out.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(anyhow!("content.xml invalide: {}", e)),
        }
    }
    Ok(out.trim().to_string())
}

// ─── Pipeline générique documents (pivot texte) ───────────────────────────────

/// Markdown → texte brut via pulldown-cmark (syntaxe supprimée).
fn md_strip(md: &str) -> String {
    use pulldown_cmark::{Event, Parser, TagEnd};
    let mut out = String::new();
    for ev in Parser::new(md) {
        match ev {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Heading(_))
            | Event::End(TagEnd::Item)
            | Event::End(TagEnd::CodeBlock) => out.push('\n'),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Extraction texte de n'importe quel format document supporté.
pub fn doc_to_text(input_path: &str, ext: &str) -> Result<String> {
    match ext {
        "txt" => std::fs::read_to_string(input_path)
            .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e)),
        "md" | "markdown" => {
            let raw = std::fs::read_to_string(input_path)
                .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
            Ok(md_strip(&raw))
        }
        "html" | "htm" => {
            let raw = std::fs::read_to_string(input_path)
                .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
            Ok(strip_html_tags(&raw).trim().to_string())
        }
        "pdf" => crate::pdf_engine::extract_text_from_pdf(input_path),
        "docx" | "doc" => crate::office_engine::docx_to_text(input_path),
        "pptx" | "ppt" => crate::office_engine::pptx_to_text(input_path),
        "rtf" => {
            let raw = std::fs::read_to_string(input_path)
                .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
            crate::data_engine::rtf_extract(&raw)
        }
        "epub" => epub_text_string(input_path),
        "odt" | "odp" => odf_to_text(input_path),
        _ => Err(anyhow!("Extraction texte non supportée pour .{}", ext)),
    }
}

/// Écriture du texte extrait vers n'importe quel format de sortie document.
pub fn text_to_doc(text: &str, title: &str, output_path: &str, fmt: &str) -> Result<()> {
    match fmt {
        "txt" | "md" => std::fs::write(output_path, text)
            .map_err(|e| anyhow!("Écriture '{}': {}", output_path, e)),
        "html" => std::fs::write(output_path, text_to_html_string(title, text))
            .map_err(|e| anyhow!("Écriture '{}': {}", output_path, e)),
        "pdf" => crate::text_engine::create_pdf_from_text(text, output_path),
        "docx" => text_to_docx(text, output_path),
        "epub" => {
            let body: String = text
                .split("\n\n")
                .map(|b| format!("<p>{}</p>\n", html_escape(b.trim()).replace('\n', "<br/>\n")))
                .collect();
            build_epub(title, &body, output_path)
        }
        "rtf" => write_rtf(text, output_path),
        "odt" => write_odt(text, output_path),
        _ => Err(anyhow!("Format de sortie document non supporté: {}", fmt)),
    }
}
