use anyhow::{anyhow, Result};
use printpdf::{
    BuiltinFont, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, TextItem,
};
use std::fs::File;
use std::io::BufWriter;

// ── Constantes PDF (printpdf attend des f32) ───────────────────────────────────

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN: f32 = 18.0;
const FONT_SIZE: f32 = 10.0;
const LINE_H_MM: f32 = 5.0; // hauteur de ligne en mm
pub(crate) const MM_TO_PT: f32 = 2.834_646; // 72 / 25.4

fn lines_per_page() -> usize {
    ((PAGE_H - 2.0 * MARGIN) / LINE_H_MM) as usize
}

// ── Word wrap à 95 colonnes (Courier monospace) ────────────────────────────────

fn char_byte_index(s: &str, char_count: usize) -> usize {
    s.char_indices().nth(char_count).map(|(i, _)| i).unwrap_or(s.len())
}

/// Découpe le texte en lignes de `max_cols` colonnes max (safe UTF-8).
pub(crate) fn wrap_text(text: &str, max_cols: usize) -> Vec<String> {
    let mut result = Vec::new();
    for raw_line in text.lines() {
        if raw_line.chars().count() <= max_cols {
            result.push(raw_line.to_string());
        } else {
            let mut remaining = raw_line;
            while remaining.chars().count() > max_cols {
                let safe_end = char_byte_index(remaining, max_cols);
                let cut = remaining[..safe_end].rfind(' ').unwrap_or(safe_end);
                result.push(remaining[..cut].to_string());
                remaining = remaining[cut..].trim_start();
            }
            if !remaining.is_empty() {
                result.push(remaining.to_string());
            }
        }
    }
    result
}

fn wrap_lines(text: &str) -> Vec<String> {
    wrap_text(text, 95)
}

// ── Helper : construit une page PDF à partir de lignes de texte ───────────────
/// Page A4-like remplie de `lines` en Courier, coin haut-gauche à `margin`.
pub(crate) fn text_page(
    lines: &[String],
    page_w: f32,
    page_h: f32,
    margin: f32,
    font_pt: f32,
    line_mm: f32,
) -> PdfPage {
    let font = PdfFontHandle::Builtin(BuiltinFont::Courier);
    let mut ops = vec![
        Op::StartTextSection,
        Op::SetFont { font, size: Pt(font_pt) },
        Op::SetLineHeight { lh: Pt(line_mm * MM_TO_PT) },
        Op::SetTextCursor {
            pos: Point { x: Mm(margin).into(), y: Mm(page_h - margin).into() },
        },
    ];
    for line in lines {
        ops.push(Op::ShowText { items: vec![TextItem::Text(line.clone())] });
        ops.push(Op::AddLineBreak);
    }
    ops.push(Op::EndTextSection);
    PdfPage::new(Mm(page_w), Mm(page_h), ops)
}

/// Écrit `doc` sur disque (printpdf 0.12 sérialise en mémoire puis on stream).
pub(crate) fn save_pdf(doc: &PdfDocument, output_path: &str) -> Result<()> {
    let file = File::create(output_path).map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let mut w = BufWriter::new(file);
    let mut warnings = Vec::new();
    doc.save_writer(&mut w, &PdfSaveOptions::default(), &mut warnings);
    std::io::Write::flush(&mut w).map_err(|e| anyhow!("Sauvegarde PDF: {}", e))?;
    Ok(())
}

// ── TXT → PDF ─────────────────────────────────────────────────────────────────

pub fn txt_to_pdf(input_path: &str, output_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;
    create_pdf_from_text(&raw, output_path)
}

pub fn create_pdf_from_text(text: &str, output_path: &str) -> Result<()> {
    let all_lines = wrap_lines(text);
    let lpp = lines_per_page();
    let chunks: Vec<&[String]> = all_lines.chunks(lpp).collect();

    // Fichier vide → une page blanche.
    let pages: Vec<_> = if chunks.is_empty() {
        vec![text_page(&[], PAGE_W, PAGE_H, MARGIN, FONT_SIZE, LINE_H_MM)]
    } else {
        chunks
            .iter()
            .map(|c| text_page(c, PAGE_W, PAGE_H, MARGIN, FONT_SIZE, LINE_H_MM))
            .collect()
    };

    let mut doc = PdfDocument::new("UniversalConverter");
    doc.with_pages(pages);
    save_pdf(&doc, output_path)
}

// ── Markdown → HTML ────────────────────────────────────────────────────────────

pub fn md_to_html(input_path: &str, output_path: &str) -> Result<()> {
    use pulldown_cmark::{html, Options, Parser};

    let md = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let parser = Parser::new_ext(&md, Options::all());
    let mut body = String::new();
    html::push_html(&mut body, parser);

    let full = format!(
        "<!DOCTYPE html>\n<html>\n<head>\
        <meta charset=\"UTF-8\">\
        <style>body{{font-family:sans-serif;max-width:800px;margin:auto;padding:2rem}}</style>\
        </head>\n<body>\n{}\n</body>\n</html>",
        body
    );

    std::fs::write(output_path, full)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;

    Ok(())
}

// ── Markdown → TXT (stripping) ─────────────────────────────────────────────────

pub fn md_to_txt(input_path: &str, output_path: &str) -> Result<()> {
    let text = extract_text_from_md(input_path)?;
    std::fs::write(output_path, text)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

fn extract_text_from_md(input_path: &str) -> Result<String> {
    use pulldown_cmark::{Event, Options, Parser, TagEnd};

    let md = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let parser = Parser::new_ext(&md, Options::all());
    let mut out = String::new();

    for event in parser {
        match event {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::End(TagEnd::Heading(_)) => out.push_str("\n\n"),
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::End(TagEnd::CodeBlock) => out.push('\n'),
            _ => {}
        }
    }

    Ok(out.trim().to_string())
}

// ── Markdown → PDF ─────────────────────────────────────────────────────────────

pub fn md_to_pdf(input_path: &str, output_path: &str) -> Result<()> {
    let text = extract_text_from_md(input_path)?;
    create_pdf_from_text(&text, output_path)
}

// ── HTML → TXT ─────────────────────────────────────────────────────────────────

pub fn html_to_txt(input_path: &str, output_path: &str) -> Result<()> {
    let content = extract_text_from_html(input_path)?;
    std::fs::write(output_path, content)
        .map_err(|e| anyhow!("Ecriture '{}': {}", output_path, e))?;
    Ok(())
}

fn extract_text_from_html(input_path: &str) -> Result<String> {
    let html = std::fs::read_to_string(input_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", input_path, e))?;

    let mut result = String::new();
    let mut in_tag = false;
    let mut in_skip = false; // script / style / head → on ignore le contenu
    let mut tag_buf = String::new();

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' => {
                let tag = tag_buf.trim().to_lowercase();
                // Balises dont le contenu est à ignorer
                if tag.starts_with("script") || tag.starts_with("style") || tag.starts_with("head") {
                    in_skip = true;
                } else if tag.starts_with("/script") || tag.starts_with("/style") || tag.starts_with("/head") {
                    in_skip = false;
                } else if !in_skip {
                    // Balises structurelles → retour à la ligne
                    if tag.starts_with("br") || tag.starts_with("p") || tag.starts_with("/p")
                        || tag.starts_with("div") || tag.starts_with("/div")
                        || tag.starts_with("li") || tag.starts_with("h1") || tag.starts_with("h2")
                        || tag.starts_with("h3") || tag.starts_with("h4") || tag.starts_with("h5")
                        || tag.starts_with("h6") || tag.starts_with("tr")
                    {
                        result.push('\n');
                    }
                }
                in_tag = false;
            }
            _ if in_tag => tag_buf.push(ch),
            _ if !in_skip => result.push(ch),
            _ => {}
        }
    }

    // Normalise les espaces multiples
    let cleaned: String = result
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(cleaned)
}

// ── HTML → PDF ─────────────────────────────────────────────────────────────────

pub fn html_to_pdf(input_path: &str, output_path: &str) -> Result<()> {
    let text = extract_text_from_html(input_path)?;
    create_pdf_from_text(&text, output_path)
}
