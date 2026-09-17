//! Filigrane des documents EN GARDANT LEUR FORMAT.
//!
//! - Conteneurs (DOCX, PPTX, XLSX, ODT, ODS, ODP, EPUB) : le calque PNG rendu
//!   par le studio est ajouté comme image dans le fichier (en-tête Word, image
//!   sur chaque diapositive, fond de feuille, fond de page OpenDocument…).
//! - HTML / SVG : calque intégré en data URL par-dessus le contenu.
//! - Texte pur (TXT, MD, YAML, TOML, XML, RTF) : aucun calque possible, le texte
//!   du filigrane est inscrit dans la syntaxe du format.

use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};

const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const IMG: &str = "ucwm.png";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_HEADER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";

// ── Paquet ZIP en mémoire ─────────────────────────────────────────────────────

struct Package {
    order: Vec<String>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    fn open(path: &str) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|e| anyhow!("Ouverture '{}': {}", path, e))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| anyhow!("Document illisible (ZIP): {}", e))?;
        let mut order = Vec::new();
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut e = zip.by_index(i).map_err(|e| anyhow!("Entrée {}: {}", i, e))?;
            if e.is_dir() {
                continue;
            }
            total = total.saturating_add(e.size());
            if total > MAX_TOTAL_BYTES {
                return Err(anyhow!("Document trop volumineux une fois décompressé"));
            }
            let mut data = Vec::new();
            e.by_ref().take(MAX_TOTAL_BYTES).read_to_end(&mut data)?;
            order.push(e.name().to_string());
            files.insert(e.name().to_string(), data);
        }
        Ok(Self { order, files })
    }

    fn text(&self, name: &str) -> Option<String> {
        self.files.get(name).map(|d| String::from_utf8_lossy(d).into_owned())
    }

    fn put(&mut self, name: &str, data: Vec<u8>) {
        if !self.files.contains_key(name) {
            self.order.push(name.to_string());
        }
        self.files.insert(name.to_string(), data);
    }

    /// `mimetype` (ODF, EPUB) doit rester la première entrée, non compressée.
    fn save(&self, path: &str) -> Result<()> {
        let file = std::fs::File::create(path).map_err(|e| anyhow!("Création '{}': {}", path, e))?;
        let mut w = zip::ZipWriter::new(file);
        let stored: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
        let mut names: Vec<&String> = self.order.iter().collect();
        names.sort_by_key(|n| n.as_str() != "mimetype");
        for name in names {
            let opts = if name == "mimetype" || name.ends_with(".png") { stored } else { deflated };
            w.start_file(name.as_str(), opts).map_err(|e| anyhow!("Écriture '{}': {}", name, e))?;
            w.write_all(&self.files[name])?;
        }
        w.finish().map_err(|e| anyhow!("Finalisation: {}", e))?;
        Ok(())
    }
}

// ── Petits outils XML (manipulation textuelle ciblée) ─────────────────────────

/// Valeur d'un attribut dans une balise ouvrante (`name="…"`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// Balise ouvrante complète commençant par `<prefix` (ex. `<w:pgSz`).
fn open_tag<'a>(xml: &'a str, prefix: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = xml[from..].find(prefix) {
        let s = from + i;
        let after = xml[s + prefix.len()..].chars().next();
        if matches!(after, Some(' ' | '/' | '>' | '\n' | '\r' | '\t')) {
            let e = xml[s..].find('>')? + s;
            return Some(&xml[s..=e]);
        }
        from = s + prefix.len();
    }
    None
}

/// Déclare un espace de noms sur l'élément racine s'il manque.
fn ensure_ns(xml: &str, root: &str, prefix: &str, uri: &str) -> String {
    let decl = format!("xmlns:{prefix}=");
    let Some(tag) = open_tag(xml, root) else { return xml.to_string() };
    if tag.contains(&decl) {
        return xml.to_string();
    }
    let at = xml.find(tag).unwrap_or(0) + root.len();
    format!("{} xmlns:{prefix}=\"{uri}\"{}", &xml[..at], &xml[at..])
}

/// Ajoute une relation (crée le fichier .rels s'il n'existe pas) et renvoie son Id.
fn add_relationship(pkg: &mut Package, rels_path: &str, rel_type: &str, target: &str) -> String {
    let xml = pkg.text(rels_path).unwrap_or_else(|| {
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".into()
    });
    // Relation déjà posée par un passage précédent sur la même partie.
    if let Some(pos) = xml.find(&format!("Target=\"{target}\"")) {
        if let Some(tag_start) = xml[..pos].rfind('<') {
            if let Some(id) = attr(&xml[tag_start..pos + target.len() + 9], "Id") {
                return id;
            }
        }
    }
    let mut n = 1;
    while xml.contains(&format!("Id=\"rIdUCWM{n}\"")) {
        n += 1;
    }
    let id = format!("rIdUCWM{n}");
    let rel = format!("<Relationship Id=\"{id}\" Type=\"{rel_type}\" Target=\"{target}\"/>");
    let updated = match xml.rfind("</Relationships>") {
        Some(i) => format!("{}{}{}", &xml[..i], rel, &xml[i..]),
        None => xml.replace("/>", &format!(">{rel}</Relationships>")),
    };
    pkg.put(rels_path, updated.into_bytes());
    id
}

fn rels_of(part: &str) -> String {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{dir}/_rels/{file}.rels")
}

fn ensure_png_content_type(pkg: &mut Package) {
    if let Some(ct) = pkg.text("[Content_Types].xml") {
        if !ct.to_lowercase().contains("extension=\"png\"") {
            let updated = ct.replacen("<Default ", "<Default Extension=\"png\" ContentType=\"image/png\"/><Default ", 1);
            pkg.put("[Content_Types].xml", updated.into_bytes());
        }
    }
}

/// Longueur ODF (`21cm`, `8.5in`, `210mm`, `595pt`) en points.
fn odf_len_pt(v: &str) -> Option<f32> {
    let v = v.trim();
    let split = v.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, unit) = v.split_at(split);
    let n: f32 = n.parse().ok()?;
    Some(match unit {
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" => n * 72.0,
        "pt" => n,
        "pc" => n * 12.0,
        _ => return None,
    })
}

// ── Taille de page (pour que le studio rende le calque au bon ratio) ──────────

/// Taille de page en points dans l'orientation du document.
pub fn page_size(path: &str, ext: &str) -> Result<(f32, f32)> {
    const A4: (f32, f32) = (595.28, 841.89);
    match ext {
        "docx" => {
            let pkg = Package::open(path)?;
            let doc = pkg.text("word/document.xml").ok_or_else(|| anyhow!("DOCX sans document.xml"))?;
            Ok(open_tag(&doc, "<w:pgSz")
                .and_then(|t| Some((attr(t, "w:w")?.parse::<f32>().ok()? / 20.0, attr(t, "w:h")?.parse::<f32>().ok()? / 20.0)))
                .unwrap_or(A4))
        }
        "pptx" => {
            let pkg = Package::open(path)?;
            let p = pkg.text("ppt/presentation.xml").ok_or_else(|| anyhow!("PPTX sans presentation.xml"))?;
            Ok(open_tag(&p, "<p:sldSz")
                .and_then(|t| Some((attr(t, "cx")?.parse::<f32>().ok()? / 12700.0, attr(t, "cy")?.parse::<f32>().ok()? / 12700.0)))
                .unwrap_or((960.0, 540.0)))
        }
        "odt" | "ods" | "odp" => {
            let pkg = Package::open(path)?;
            let styles = pkg.text("styles.xml").unwrap_or_default();
            Ok(open_tag(&styles, "<style:page-layout-properties")
                .and_then(|t| Some((odf_len_pt(&attr(t, "fo:page-width")?)?, odf_len_pt(&attr(t, "fo:page-height")?)?)))
                .unwrap_or(if ext == "odp" { (793.7, 595.3) } else { A4 }))
        }
        // Fond de feuille Excel : tuile ; HTML/EPUB : écran.
        "xlsx" => Ok((1200.0, 800.0)),
        "html" | "htm" => Ok((1600.0, 900.0)),
        "epub" => Ok((600.0, 900.0)),
        "svg" => {
            let img = crate::conversion_engine::svg_to_dynamic_image(path)?;
            Ok((img.width() as f32, img.height() as f32))
        }
        _ => Err(anyhow!("Taille de page inconnue pour .{ext}")),
    }
}

// ── Calque visuel, format conservé ────────────────────────────────────────────

pub fn apply_visual(path: &str, ext: &str, png: &[u8], blend: &str, output: &str) -> Result<()> {
    match ext {
        "html" | "htm" => {
            let src = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
            std::fs::write(output, html_with_overlay(&src, png, blend))?;
            Ok(())
        }
        "svg" => {
            let src = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
            std::fs::write(output, svg_with_overlay(&src, png, blend)?)?;
            Ok(())
        }
        "docx" | "pptx" | "xlsx" | "odt" | "ods" | "odp" | "epub" => {
            let mut pkg = Package::open(path)?;
            match ext {
                "docx" => docx(&mut pkg, png)?,
                "pptx" => pptx(&mut pkg, png)?,
                "xlsx" => xlsx(&mut pkg, png)?,
                "odp" => odp(&mut pkg, png)?,
                "epub" => epub(&mut pkg, png)?,
                _ => odf_page_background(&mut pkg, png)?,
            }
            pkg.save(output)
        }
        _ => Err(anyhow!("Filigrane visuel non disponible pour .{ext}")),
    }
}

fn data_url(png: &[u8]) -> String {
    use base64::Engine;
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

fn html_with_overlay(src: &str, png: &[u8], blend: &str) -> String {
    // mix-blend-mode : le calque altère vraiment les pixels de la page, comme dans l'aperçu.
    let img = format!(
        "<img alt=\"\" aria-hidden=\"true\" data-uc-filigrane src=\"{}\" style=\"position:fixed;inset:0;width:100vw;height:100vh;object-fit:contain;pointer-events:none;z-index:2147483647;mix-blend-mode:{}\">",
        data_url(png), blend
    );
    match src.to_lowercase().rfind("</body>") {
        Some(i) => format!("{}{}{}", &src[..i], img, &src[i..]),
        None => format!("{src}{img}"),
    }
}

fn svg_with_overlay(src: &str, png: &[u8], blend: &str) -> Result<String> {
    let tag = open_tag(src, "<svg").ok_or_else(|| anyhow!("SVG sans élément <svg>"))?;
    let (x, y, w, h) = match attr(tag, "viewBox") {
        Some(vb) => {
            let v: Vec<f32> = vb.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|p| p.parse().ok()).collect();
            if v.len() != 4 { return Err(anyhow!("viewBox SVG invalide")); }
            (v[0], v[1], v[2], v[3])
        }
        None => {
            let num = |a: &str| attr(tag, a).and_then(|s| s.trim_end_matches("px").parse::<f32>().ok());
            (0.0, 0.0, num("width").unwrap_or(300.0), num("height").unwrap_or(150.0))
        }
    };
    let image = format!(
        "<image data-uc-filigrane=\"1\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" style=\"mix-blend-mode:{}\" href=\"{}\"/>",
        blend, data_url(png)
    );
    let end = src.rfind("</svg>").ok_or_else(|| anyhow!("SVG sans </svg>"))?;
    Ok(format!("{}{}{}", &src[..end], image, &src[end..]))
}

/// Word : image ancrée sur la page dans l'en-tête de chaque section.
fn docx(pkg: &mut Package, png: &[u8]) -> Result<()> {
    let doc = pkg.text("word/document.xml").ok_or_else(|| anyhow!("DOCX sans document.xml"))?;
    let (w_twip, h_twip) = open_tag(&doc, "<w:pgSz")
        .and_then(|t| Some((attr(t, "w:w")?.parse::<i64>().ok()?, attr(t, "w:h")?.parse::<i64>().ok()?)))
        .unwrap_or((11906, 16838));
    let (cx, cy) = (w_twip * 635, h_twip * 635);
    pkg.put(&format!("word/media/{IMG}"), png.to_vec());
    ensure_png_content_type(pkg);

    let drawing = |rid: &str| format!(
        "<w:p><w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"251659264\" behindDoc=\"1\" locked=\"1\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"page\"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/><wp:docPr id=\"99001\" name=\"UC Filigrane\"/><wp:cNvGraphicFramePr/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"99001\" name=\"{IMG}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>"
    );

    // 1. En-têtes existants référencés par les sections : on y ajoute l'image.
    let doc_rels = pkg.text("word/_rels/document.xml.rels").unwrap_or_default();
    let mut referenced: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(i) = doc[from..].find("<w:headerReference") {
        let s = from + i;
        let tag = open_tag(&doc[s..], "<w:headerReference").unwrap_or("");
        if let Some(id) = attr(tag, "r:id") {
            if let Some(target) = rel_target(&doc_rels, &id) {
                let part = format!("word/{}", target.trim_start_matches('/').trim_start_matches("word/"));
                if !referenced.contains(&part) { referenced.push(part); }
            }
        }
        from = s + 1;
    }
    for part in &referenced {
        let Some(xml) = pkg.text(part) else { continue };
        let rid = add_relationship(pkg, &rels_of(part), REL_IMAGE, &format!("media/{IMG}"));
        let xml = ensure_ns(&ensure_ns(&xml, "<w:hdr", "r", NS_R), "<w:hdr", "wp", NS_WP);
        let end = xml.rfind("</w:hdr>").ok_or_else(|| anyhow!("En-tête Word invalide: {part}"))?;
        pkg.put(part, format!("{}{}{}", &xml[..end], drawing(&rid), &xml[end..]).into_bytes());
    }

    // 2. Première section sans en-tête par défaut (ni page de titre) : en-tête créé.
    //    Les sections suivantes sans référence héritent de la précédente.
    let first_sect_start = doc.find("<w:sectPr").ok_or_else(|| anyhow!("DOCX sans section"))?;
    let first_sect_end = doc[first_sect_start..].find("</w:sectPr>").map(|i| first_sect_start + i).unwrap_or(first_sect_start);
    let first_sect = &doc[first_sect_start..first_sect_end];
    let mut missing = Vec::new();
    if !first_sect.contains("w:type=\"default\"") { missing.push("default"); }
    if first_sect.contains("<w:titlePg") && !first_sect.contains("w:type=\"first\"") { missing.push("first"); }
    let even_on = pkg.text("word/settings.xml").is_some_and(|s| s.contains("<w:evenAndOddHeaders"));
    if even_on && !first_sect.contains("w:type=\"even\"") { missing.push("even"); }

    if !missing.is_empty() {
        let part = "word/headerUCWM.xml";
        let img_rid = add_relationship(pkg, &rels_of(part), REL_IMAGE, &format!("media/{IMG}"));
        pkg.put(part, format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:hdr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"{NS_R}\" xmlns:wp=\"{NS_WP}\">{}</w:hdr>",
            drawing(&img_rid)
        ).into_bytes());
        let hdr_rid = add_relationship(pkg, "word/_rels/document.xml.rels", REL_HEADER, "headerUCWM.xml");
        if let Some(ct) = pkg.text("[Content_Types].xml") {
            if !ct.contains("/word/headerUCWM.xml") {
                let ov = "<Override PartName=\"/word/headerUCWM.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>";
                pkg.put("[Content_Types].xml", ct.replacen("</Types>", &format!("{ov}</Types>"), 1).into_bytes());
            }
        }
        let refs: String = missing.iter().map(|t| format!("<w:headerReference w:type=\"{t}\" r:id=\"{hdr_rid}\"/>")).collect();
        // Les références d'en-tête ouvrent la section (ordre imposé par le schéma).
        let tag = open_tag(&doc[first_sect_start..], "<w:sectPr").unwrap_or("<w:sectPr>");
        let insert_at = first_sect_start + tag.len();
        let doc = if tag.ends_with("/>") {
            format!("{}<w:sectPr>{refs}</w:sectPr>{}", &doc[..first_sect_start], &doc[insert_at..])
        } else {
            format!("{}{refs}{}", &doc[..insert_at], &doc[insert_at..])
        };
        pkg.put("word/document.xml", ensure_ns(&doc, "<w:document", "r", NS_R).into_bytes());
    }
    Ok(())
}

fn rel_target(rels: &str, id: &str) -> Option<String> {
    let pos = rels.find(&format!("Id=\"{id}\""))?;
    let start = rels[..pos].rfind('<')?;
    let end = rels[pos..].find('>')? + pos;
    attr(&rels[start..=end], "Target")
}

/// PowerPoint : image pleine diapositive au premier plan de chaque diapositive.
fn pptx(pkg: &mut Package, png: &[u8]) -> Result<()> {
    let pres = pkg.text("ppt/presentation.xml").ok_or_else(|| anyhow!("PPTX sans presentation.xml"))?;
    let (cx, cy) = open_tag(&pres, "<p:sldSz")
        .and_then(|t| Some((attr(t, "cx")?, attr(t, "cy")?)))
        .unwrap_or(("12192000".into(), "6858000".into()));
    pkg.put(&format!("ppt/media/{IMG}"), png.to_vec());
    ensure_png_content_type(pkg);

    let slides: Vec<String> = pkg.files.keys()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .cloned().collect();
    if slides.is_empty() { return Err(anyhow!("Présentation sans diapositive")); }
    for part in slides {
        let xml = pkg.text(&part).unwrap_or_default();
        let rid = add_relationship(pkg, &rels_of(&part), REL_IMAGE, &format!("../media/{IMG}"));
        let pic = format!(
            "<p:pic><p:nvPicPr><p:cNvPr id=\"99001\" name=\"UC Filigrane\"/><p:cNvPicPr><a:picLocks noGrp=\"1\" noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr userDrawn=\"1\"/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"
        );
        let end = xml.rfind("</p:spTree>").ok_or_else(|| anyhow!("Diapositive invalide: {part}"))?;
        let xml = format!("{}{}{}", &xml[..end], pic, &xml[end..]);
        let xml = ensure_ns(&ensure_ns(&xml, "<p:sld", "r", NS_R), "<p:sld", "a", "http://schemas.openxmlformats.org/drawingml/2006/main");
        pkg.put(&part, xml.into_bytes());
    }
    Ok(())
}

/// Excel : image de fond de chaque feuille (affichée à l'écran, en mosaïque).
fn xlsx(pkg: &mut Package, png: &[u8]) -> Result<()> {
    pkg.put(&format!("xl/media/{IMG}"), png.to_vec());
    ensure_png_content_type(pkg);
    let sheets: Vec<String> = pkg.files.keys()
        .filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
        .cloned().collect();
    if sheets.is_empty() { return Err(anyhow!("Classeur sans feuille")); }
    for part in sheets {
        let xml = pkg.text(&part).unwrap_or_default();
        if xml.contains("<picture ") { continue; } // fond déjà défini par l'auteur
        let rid = add_relationship(pkg, &rels_of(&part), REL_IMAGE, &format!("../media/{IMG}"));
        let pic = format!("<picture r:id=\"{rid}\"/>");
        // Position imposée par le schéma : avant ces éléments s'ils existent.
        let at = ["<oleObjects", "<controls", "<webPublishItems", "<tableParts", "<extLst", "</worksheet>"]
            .iter().filter_map(|t| xml.find(t)).min()
            .ok_or_else(|| anyhow!("Feuille invalide: {part}"))?;
        let xml = format!("{}{}{}", &xml[..at], pic, &xml[at..]);
        pkg.put(&part, ensure_ns(&xml, "<worksheet", "r", NS_R).into_bytes());
    }
    Ok(())
}

fn odf_manifest_png(pkg: &mut Package, png: &[u8]) {
    pkg.put(&format!("Pictures/{IMG}"), png.to_vec());
    if let Some(m) = pkg.text("META-INF/manifest.xml") {
        if !m.contains(&format!("Pictures/{IMG}")) {
            let entry = format!("<manifest:file-entry manifest:full-path=\"Pictures/{IMG}\" manifest:media-type=\"image/png\"/>");
            pkg.put("META-INF/manifest.xml", m.replacen("</manifest:manifest>", &format!("{entry}</manifest:manifest>"), 1).into_bytes());
        }
    }
}

/// ODT / ODS : image de fond étirée sur chaque mise en page.
fn odf_page_background(pkg: &mut Package, png: &[u8]) -> Result<()> {
    odf_manifest_png(pkg, png);
    let styles = ensure_page_layout(pkg.text("styles.xml").unwrap_or_default())?;
    let bg =format!("<style:background-image xlink:href=\"Pictures/{IMG}\" xlink:type=\"simple\" xlink:actuate=\"onLoad\" style:repeat=\"stretch\"/>");
    let mut out = String::with_capacity(styles.len() + 512);
    let mut rest = styles.as_str();
    let mut count = 0;
    while let Some(tag) = open_tag(rest, "<style:page-layout-properties") {
        let s = rest.find(tag).unwrap_or(0);
        out.push_str(&rest[..s]);
        if let Some(open) = tag.strip_suffix("/>") {
            out.push_str(open);
            out.push('>');
            out.push_str(&bg);
            out.push_str("</style:page-layout-properties>");
            rest = &rest[s + tag.len()..];
        } else {
            out.push_str(tag);
            let body_start = s + tag.len();
            let close = rest[body_start..].find("</style:page-layout-properties>").map(|i| body_start + i)
                .ok_or_else(|| anyhow!("styles.xml invalide"))?;
            let body = &rest[body_start..close];
            // Remplacer un fond image existant plutôt que d'en empiler deux.
            let body = match (body.find("<style:background-image"), body.find("</style:background-image>")) {
                (Some(a), Some(b)) => format!("{}{}", &body[..a], &body[b + "</style:background-image>".len()..]),
                (Some(a), None) => {
                    let e = body[a..].find("/>").map(|i| a + i + 2).unwrap_or(a);
                    format!("{}{}", &body[..a], &body[e..])
                }
                _ => body.to_string(),
            };
            out.push_str(&bg);
            out.push_str(&body);
            rest = &rest[close..];
        }
        count += 1;
    }
    out.push_str(rest);
    if count == 0 { return Err(anyhow!("Aucune mise en page dans styles.xml")); }
    let out = ensure_ns(&out, "<office:document-styles", "xlink", "http://www.w3.org/1999/xlink");
    pkg.put("styles.xml", out.into_bytes());
    Ok(())
}

/// Document sans mise en page (généré par un outil minimal) : on ajoute la page
/// A4 par défaut « Standard », celle que LibreOffice appliquerait de toute façon.
fn ensure_page_layout(styles: String) -> Result<String> {
    if styles.contains("<style:page-layout-properties") {
        return Ok(styles);
    }
    const NS: [(&str, &str); 5] = [
        ("office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
        ("style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
        ("fo", "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"),
        ("xlink", "http://www.w3.org/1999/xlink"),
        ("draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
    ];
    let layout = "<style:page-layout style:name=\"UCWMLayout\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" fo:margin-top=\"2cm\" fo:margin-bottom=\"2cm\" fo:margin-left=\"2cm\" fo:margin-right=\"2cm\"/></style:page-layout>";
    if styles.contains("<style:master-page") {
        // ponytail: pages maîtresses sans mise en page déclarée = document hors norme, non retouché.
        return Err(anyhow!("styles.xml : pages maîtresses sans mise en page"));
    }
    let master = "<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"UCWMLayout\"/></office:master-styles>";

    let root_tag = open_tag(&styles, "<office:document-styles");
    let mut xml = match root_tag {
        Some(tag) if tag.ends_with("/>") => styles.replacen(tag, &format!("{}></office:document-styles>", &tag[..tag.len() - 2]), 1),
        Some(_) => styles,
        None => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles office:version=\"1.2\"></office:document-styles>".to_string(),
    };
    for (p, uri) in NS {
        xml = ensure_ns(&xml, "<office:document-styles", p, uri);
    }
    let auto = match xml.find("</office:automatic-styles>") {
        Some(i) => format!("{}{layout}{}", &xml[..i], &xml[i..]),
        None => {
            let at = xml.find("<office:master-styles").or_else(|| xml.rfind("</office:document-styles>"))
                .ok_or_else(|| anyhow!("styles.xml invalide"))?;
            format!("{}<office:automatic-styles>{layout}</office:automatic-styles>{}", &xml[..at], &xml[at..])
        }
    };
    let at = match auto.find("</office:master-styles>") {
        Some(i) => return Ok(format!("{}{}{}", &auto[..i], "<style:master-page style:name=\"Standard\" style:page-layout-name=\"UCWMLayout\"/>", &auto[i..])),
        None => auto.rfind("</office:document-styles>").ok_or_else(|| anyhow!("styles.xml invalide"))?,
    };
    Ok(format!("{}{master}{}", &auto[..at], &auto[at..]))
}

/// ODP : cadre image pleine page au premier plan de chaque diapositive.
fn odp(pkg: &mut Package, png: &[u8]) -> Result<()> {
    odf_manifest_png(pkg, png);
    let styles = pkg.text("styles.xml").unwrap_or_default();
    let (w, h) = open_tag(&styles, "<style:page-layout-properties")
        .and_then(|t| Some((attr(t, "fo:page-width")?, attr(t, "fo:page-height")?)))
        .unwrap_or(("28cm".into(), "21cm".into()));
    let mut content = pkg.text("content.xml").ok_or_else(|| anyhow!("ODP sans content.xml"))?;
    // Diapositive vide auto-fermante : l'ouvrir pour pouvoir y placer le cadre.
    let mut from = 0;
    while let Some(tag) = open_tag(&content[from..], "<draw:page").map(str::to_string) {
        let at = from + content[from..].find(&tag).unwrap_or(0);
        if tag.ends_with("/>") {
            content.replace_range(at..at + tag.len(), &format!("{}></draw:page>", &tag[..tag.len() - 2]));
        }
        from = at + 1;
    }
    for (prefix, uri) in [("xlink", "http://www.w3.org/1999/xlink"), ("svg", "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0")] {
        content = ensure_ns(&content, "<office:document-content", prefix, uri);
    }
    let frame = format!("<draw:frame draw:name=\"UC Filigrane\" svg:width=\"{w}\" svg:height=\"{h}\" svg:x=\"0cm\" svg:y=\"0cm\"><draw:image xlink:href=\"Pictures/{IMG}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>");
    let mut out = String::with_capacity(content.len() + 1024);
    let mut rest = content.as_str();
    let mut count = 0;
    while let Some(end) = rest.find("</draw:page>") {
        let page = &rest[..end];
        // Les notes ferment la diapositive : le cadre se place avant elles.
        let at = page.rfind("<presentation:notes").unwrap_or(end);
        out.push_str(&rest[..at]);
        out.push_str(&frame);
        out.push_str(&rest[at..end + "</draw:page>".len()]);
        rest = &rest[end + "</draw:page>".len()..];
        count += 1;
    }
    out.push_str(rest);
    if count == 0 { return Err(anyhow!("Présentation sans diapositive")); }
    pkg.put("content.xml", out.into_bytes());
    Ok(())
}

/// EPUB : image de fond (centrée, ajustée) sur chaque chapitre XHTML.
fn epub(pkg: &mut Package, png: &[u8]) -> Result<()> {
    let container = pkg.text("META-INF/container.xml").ok_or_else(|| anyhow!("EPUB sans container.xml"))?;
    let opf_path = open_tag(&container, "<rootfile").and_then(|t| attr(t, "full-path"))
        .ok_or_else(|| anyhow!("EPUB : OPF introuvable"))?;
    let opf_dir = opf_path.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default();
    let mut opf = pkg.text(&opf_path).ok_or_else(|| anyhow!("EPUB : {opf_path} absent"))?;
    pkg.put(&format!("{opf_dir}{IMG}"), png.to_vec());
    if !opf.contains(&format!("href=\"{IMG}\"")) {
        opf = opf.replacen("</manifest>", &format!("<item id=\"uc-filigrane\" href=\"{IMG}\" media-type=\"image/png\"/></manifest>"), 1);
        pkg.put(&opf_path, opf.clone().into_bytes());
    }
    let mut count = 0;
    let mut from = 0;
    while let Some(i) = opf[from..].find("<item ") {
        let s = from + i;
        let tag = open_tag(&opf[s..], "<item").unwrap_or("");
        from = s + 1;
        if attr(tag, "media-type").as_deref() != Some("application/xhtml+xml") { continue; }
        let Some(href) = attr(tag, "href") else { continue };
        let part = format!("{opf_dir}{href}");
        let Some(xhtml) = pkg.text(&part) else { continue };
        let depth = href.matches('/').count();
        let url = format!("{}{IMG}", "../".repeat(depth));
        let style = format!("<style data-uc-filigrane=\"1\">html,body{{background-image:url(\"{url}\") !important;background-repeat:no-repeat !important;background-position:center !important;background-size:contain !important;background-attachment:fixed !important}}</style>");
        let updated = match xhtml.find("</head>") {
            Some(h) => format!("{}{}{}", &xhtml[..h], style, &xhtml[h..]),
            None => continue,
        };
        pkg.put(&part, updated.into_bytes());
        count += 1;
    }
    if count == 0 { return Err(anyhow!("EPUB sans chapitre XHTML")); }
    Ok(())
}

// ── Texte pur : le texte du filigrane dans la syntaxe du format ───────────────

pub fn apply_textual(path: &str, ext: &str, text: &str, output: &str) -> Result<()> {
    let src = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() { return Err(anyhow!("Texte du filigrane vide")); }
    let one = lines.join(" · ");
    let out = match ext {
        "txt" => format!("{one}\n\n{src}\n\n{one}\n"),
        "md" | "markdown" => format!("> **{}**\n\n{src}\n\n> **{}**\n", md_escape(&one), md_escape(&one)),
        "yaml" | "yml" | "toml" => {
            let c: String = lines.iter().map(|l| format!("# {l}\n")).collect();
            format!("{c}{src}")
        }
        "xml" => {
            let c = format!("<!-- {} -->\n", one.replace("--", "—"));
            match (src.trim_start().starts_with("<?xml"), src.find("?>")) {
                (true, Some(i)) => format!("{}\n{c}{}", &src[..i + 2], src[i + 2..].trim_start_matches(['\r', '\n'])),
                _ => format!("{c}{src}"),
            }
        }
        "rtf" => {
            let end = src.rfind('}').ok_or_else(|| anyhow!("RTF invalide"))?;
            // Paragraphe centré en tête de document, juste après la table des polices/couleurs.
            let para = format!("\\pard\\qc\\b {}\\b0\\par\\pard ", rtf_escape(&one));
            let at = src.find("\\pard").unwrap_or(end);
            format!("{}{}{}", &src[..at], para, &src[at..])
        }
        _ => return Err(anyhow!("Filigrane texte non disponible pour .{ext}")),
    };
    std::fs::write(output, out)?;
    Ok(())
}

fn md_escape(s: &str) -> String {
    s.chars().flat_map(|c| if "\\`*_[]#<>".contains(c) { vec!['\\', c] } else { vec![c] }).collect()
}

fn rtf_escape(s: &str) -> String {
    s.chars().map(|c| match c {
        '\\' | '{' | '}' => format!("\\{c}"),
        c if (c as u32) < 128 => c.to_string(),
        c => format!("\\u{}?", c as u32 as i16),
    }).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut buf = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 128]))
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn tmp(name: &str) -> String {
        std::env::temp_dir().join(format!("uc_wmdoc_{}_{name}", std::process::id())).to_string_lossy().into_owned()
    }

    #[test]
    fn docx_garde_son_format_et_son_texte() {
        let (txt, src, out) = (tmp("a.txt"), tmp("a.docx"), tmp("a_wm.docx"));
        std::fs::write(&txt, "Bonjour le monde\nDeuxième ligne").unwrap();
        crate::doc_engine::txt_to_docx(&txt, &src).unwrap();
        let size = page_size(&src, "docx").unwrap();
        assert!(size.0 > 100.0 && size.1 > 100.0, "taille {size:?}");
        apply_visual(&src, "docx", &png(), "normal", &out).unwrap();

        let text = crate::office_engine::docx_to_text(&out).expect("DOCX relisible");
        let pkg = Package::open(&out).unwrap();
        for f in [&txt, &src, &out] { let _ = std::fs::remove_file(f); }
        assert!(text.contains("Bonjour le monde"), "texte perdu: {text}");
        assert!(pkg.files.contains_key("word/media/ucwm.png"));
        let doc = pkg.text("word/document.xml").unwrap();
        let hdr = pkg.text("word/headerUCWM.xml").unwrap();
        assert!(doc.contains("<w:headerReference w:type=\"default\""), "référence d'en-tête absente");
        assert!(hdr.contains("r:embed=\"rIdUCWM1\""), "{hdr}");
        assert!(pkg.text("word/_rels/headerUCWM.xml.rels").unwrap().contains("media/ucwm.png"));
        assert!(pkg.text("[Content_Types].xml").unwrap().contains("headerUCWM.xml"));
    }

    #[test]
    fn xlsx_reste_lisible() {
        let (csv, src, out) = (tmp("b.csv"), tmp("b.xlsx"), tmp("b_wm.xlsx"));
        std::fs::write(&csv, "nom,valeur\nalpha,1\nbeta,2\n").unwrap();
        crate::office_engine::csv_to_xlsx(&csv, &src).unwrap();
        apply_visual(&src, "xlsx", &png(), "normal", &out).unwrap();
        let csv_back = tmp("b_back.csv");
        crate::office_engine::excel_to_csv(&out, &csv_back).expect("XLSX relisible");
        let back = std::fs::read_to_string(&csv_back).unwrap();
        let pkg = Package::open(&out).unwrap();
        for f in [&csv, &src, &out, &csv_back] { let _ = std::fs::remove_file(f); }
        assert!(back.contains("beta"), "{back}");
        assert!(pkg.text("xl/worksheets/sheet1.xml").unwrap().contains("<picture r:id="));
    }

    #[test]
    fn epub_et_odt_restent_valides() {
        let txt = tmp("c.txt");
        std::fs::write(&txt, "Chapitre unique").unwrap();

        let (e_src, e_out) = (tmp("c.epub"), tmp("c_wm.epub"));
        crate::doc_engine::txt_to_epub(&txt, &e_src).unwrap();
        apply_visual(&e_src, "epub", &png(), "normal", &e_out).unwrap();
        let back = tmp("c_back.txt");
        crate::doc_engine::epub_to_text(&e_out, &back).expect("EPUB relisible");
        assert!(std::fs::read_to_string(&back).unwrap().contains("Chapitre unique"));
        let first = zip::ZipArchive::new(std::fs::File::open(&e_out).unwrap()).unwrap().by_index(0).unwrap().name().to_string();
        assert_eq!(first, "mimetype", "mimetype doit rester en tête");

        let (o_src, o_out) = (tmp("c.odt"), tmp("c_wm.odt"));
        crate::doc_engine::write_odt("Texte ODT", &o_src).unwrap();
        apply_visual(&o_src, "odt", &png(), "normal", &o_out).unwrap();
        let pkg = Package::open(&o_out).unwrap();
        for f in [&txt, &e_src, &e_out, &back, &o_src, &o_out] { let _ = std::fs::remove_file(f); }
        let styles = pkg.text("styles.xml").unwrap();
        assert!(styles.contains("xlink:href=\"Pictures/ucwm.png\""), "{styles}");
        assert!(styles.contains("<style:master-page style:name=\"Standard\" style:page-layout-name=\"UCWMLayout\"/>"), "{styles}");
        assert!(styles.contains("xmlns:style="), "espace de noms style manquant: {styles}");
        assert!(pkg.text("META-INF/manifest.xml").unwrap().contains("Pictures/ucwm.png"));
    }

    #[test]
    fn svg_et_html_gardent_leur_format() {
        let svg = svg_with_overlay("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"10 20 300 100\"><rect/></svg>", &png(), "multiply").unwrap();
        assert!(svg.contains("x=\"10\" y=\"20\" width=\"300\" height=\"100\"") && svg.contains("mix-blend-mode:multiply"), "{svg}");
        assert!(svg.ends_with("</svg>"));
        let html = html_with_overlay("<html><BODY><p>x</p></BODY></html>", &png(), "overlay");
        assert!(html.contains("<p>x</p><img") && html.ends_with("</BODY></html>"), "{html}");
    }

    #[test]
    fn formats_texte() {
        let (src, out) = (tmp("d.xml"), tmp("d_wm.xml"));
        std::fs::write(&src, "<?xml version=\"1.0\"?>\n<r/>").unwrap();
        apply_textual(&src, "xml", "© Moi -- 2026", &out).unwrap();
        let xml = std::fs::read_to_string(&out).unwrap();
        assert_eq!(xml, "<?xml version=\"1.0\"?>\n<!-- © Moi — 2026 -->\n<r/>");

        std::fs::write(&src, "a: 1\n").unwrap();
        apply_textual(&src, "yaml", "WM\nligne", &out).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "# WM\n# ligne\na: 1\n");

        std::fs::write(&src, "{\\rtf1\\ansi{\\fonttbl{\\f0 Arial;}}\\pard Bonjour\\par}").unwrap();
        apply_textual(&src, "rtf", "Été {x}", &out).unwrap();
        let rtf = std::fs::read_to_string(&out).unwrap();
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
        assert!(rtf.contains("\\pard\\qc\\b \\u201?t\\u233? \\{x\\}\\b0\\par\\pard \\pard Bonjour"), "{rtf}");
        assert_eq!(crate::data_engine::rtf_extract(&rtf).unwrap().contains("Bonjour"), true);
    }

    fn zip_of(path: &str, files: &[(&str, &str)]) {
        let mut pkg = Package { order: Vec::new(), files: BTreeMap::new() };
        for (n, d) in files { pkg.put(n, d.as_bytes().to_vec()); }
        pkg.save(path).unwrap();
    }

    #[test]
    fn pptx_et_odp_restent_lisibles() {
        let (src, out) = (tmp("e.pptx"), tmp("e_wm.pptx"));
        zip_of(&src, &[
            ("[Content_Types].xml", "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>"),
            ("ppt/presentation.xml", "<p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:sldSz cx=\"9144000\" cy=\"5143500\"/></p:presentation>"),
            ("ppt/slides/slide1.xml", "<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Diapo un</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"),
        ]);
        assert_eq!(page_size(&src, "pptx").unwrap(), (720.0, 405.0));
        apply_visual(&src, "pptx", &png(), "normal", &out).unwrap();
        let text = crate::office_engine::pptx_to_text(&out).expect("PPTX relisible");
        let pkg = Package::open(&out).unwrap();
        let slide = pkg.text("ppt/slides/slide1.xml").unwrap();
        assert!(text.contains("Diapo un"), "{text}");
        assert!(slide.contains("<a:ext cx=\"9144000\" cy=\"5143500\"/>") && slide.contains("xmlns:r="), "{slide}");
        assert!(pkg.text("ppt/slides/_rels/slide1.xml.rels").unwrap().contains("../media/ucwm.png"));
        assert!(pkg.text("[Content_Types].xml").unwrap().contains("Extension=\"png\""));

        let (osrc, oout) = (tmp("e.odp"), tmp("e_wm.odp"));
        zip_of(&osrc, &[
            ("mimetype", "application/vnd.oasis.opendocument.presentation"),
            ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\"></manifest:manifest>"),
            ("styles.xml", "<office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\"><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"28cm\" fo:page-height=\"15.75cm\"/></style:page-layout></office:automatic-styles></office:document-styles>"),
            ("content.xml", "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\"><office:body><office:presentation><draw:page draw:name=\"p1\"><draw:frame><draw:text-box><text:p>Diapo ODP</text:p></draw:text-box></draw:frame><presentation:notes><text:p>note</text:p></presentation:notes></draw:page><draw:page draw:name=\"p2\"/></office:presentation></office:body></office:document-content>"),
        ]);
        let (w, h) = page_size(&osrc, "odp").unwrap();
        assert!((w - 793.7).abs() < 0.5 && (h - 446.5).abs() < 0.5, "{w}x{h}");
        apply_visual(&osrc, "odp", &png(), "normal", &oout).unwrap();
        let text = crate::doc_engine::odf_to_text(&oout).expect("ODP relisible");
        let content = Package::open(&oout).unwrap().text("content.xml").unwrap();
        for f in [&src, &out, &osrc, &oout] { let _ = std::fs::remove_file(f); }
        assert!(text.contains("Diapo ODP"), "{text}");
        assert!(content.contains("</draw:frame><draw:frame draw:name=\"UC Filigrane\""), "cadre avant les notes: {content}");
        assert!(content.contains("svg:width=\"28cm\""), "{content}");
        assert_eq!(content.matches("draw:name=\"UC Filigrane\"").count(), 2, "chaque diapositive, y compris vide: {content}");
    }

    #[test]
    fn open_tag_ne_confond_pas_les_prefixes() {
        let xml = "<w:pgSzX a=\"1\"/><w:pgSz w:w=\"12240\" w:h=\"15840\"/>";
        assert_eq!(open_tag(xml, "<w:pgSz"), Some("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"));
    }
}
