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
    let title = file_stem(input_path);
    let body: String = text
        .split("\n\n")
        .map(|block| format!("<p>{}</p>\n", html_escape(block.trim()).replace('\n', "<br>\n")))
        .collect();
    let html = format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{}</body></html>\n",
        html_escape(&title),
        body
    );
    std::fs::write(output_path, html).map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
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
    let chapters = epub_chapters(input_path)?;
    let mut out = String::new();
    for c in &chapters {
        out.push_str(&strip_html_tags(extract_body(c)));
        out.push_str("\n\n");
    }
    std::fs::write(output_path, out.trim())
        .map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
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
