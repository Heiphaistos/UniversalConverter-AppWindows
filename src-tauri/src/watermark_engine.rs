//! Filigrane : composition d'un calque RGBA (rendu côté client sur <canvas>)
//! sur une image pleine résolution ou sur chaque page d'un PDF.
//!
//! Le calque arrive en PNG base64. Tout le style (police, dégradé, rotation,
//! ombre, répétition) est produit par le canvas du studio : ce module ne fait
//! que de la composition, il n'y a donc jamais d'écart entre aperçu et sortie.

use anyhow::{anyhow, Result};
use image::{DynamicImage, GenericImageView, ImageFormat, RgbaImage};
use lopdf::{dictionary, Document, Object, Stream};
use std::io::Cursor;

// ── Décodage du calque ────────────────────────────────────────────────────────

/// Décode un PNG base64 (avec ou sans préfixe `data:`) en image RGBA.
pub fn decode_overlay(b64: &str) -> Result<RgbaImage> {
    let bytes = overlay_png_bytes(b64)?;
    let img = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .map_err(|e| anyhow!("Calque filigrane illisible (PNG): {e}"))?;
    Ok(img.to_rgba8())
}

/// Octets PNG du calque (base64, avec ou sans préfixe `data:`), vérifiés.
pub fn overlay_png_bytes(b64: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let payload = b64.rsplit("base64,").next().unwrap_or(b64);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|e| anyhow!("Calque filigrane illisible (base64): {e}"))?;
    if !bytes.starts_with(b"\x89PNG") {
        return Err(anyhow!("Calque filigrane illisible : PNG attendu"));
    }
    Ok(bytes)
}

/// Redimensionne le calque aux dimensions cibles s'il en diffère.
fn fit_overlay(overlay: RgbaImage, w: u32, h: u32) -> RgbaImage {
    if overlay.width() == w && overlay.height() == h {
        return overlay;
    }
    DynamicImage::ImageRgba8(overlay)
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
}

// ── Aperçu : source réduite + dimensions réelles ──────────────────────────────

pub struct Preview {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// PNG base64 réduit à `max_side` pour l'aperçu, avec les dimensions d'origine.
/// Le studio dessine son calque à l'échelle réelle : l'aperçu peut être réduit
/// sans dégrader la sortie.
pub fn load_preview(path: &str, max_side: u32) -> Result<Preview> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let img = if ext == "svg" {
        crate::conversion_engine::svg_to_dynamic_image(path)?
    } else {
        image::open(path).map_err(|e| anyhow!("Ouverture '{}': {}", path, e))?
    };

    let (width, height) = img.dimensions();
    let shown = if width.max(height) > max_side {
        img.thumbnail(max_side, max_side)
    } else {
        img
    };

    let mut buf = Vec::new();
    shown
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| anyhow!("Encodage aperçu: {e}"))?;

    use base64::Engine;
    Ok(Preview {
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&buf)
        ),
        width,
        height,
    })
}

// ── Modes de fusion ───────────────────────────────────────────────────────────

/// Fusion du calque avec l'image. « Normal » se contente de la transparence ;
/// les autres modes altèrent la luminance et la chrominance des pixels d'origine,
/// ce qui rend la restauration des pixels d'origine bien plus difficile.
/// Formules séparables du standard W3C Compositing, identiques à celles du canvas
/// du navigateur et de PDF : l'aperçu et le fichier produit restent identiques.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl Blend {
    pub fn from_str(s: &str) -> Self {
        match s {
            "multiply" => Blend::Multiply,
            "screen" => Blend::Screen,
            "overlay" => Blend::Overlay,
            "darken" => Blend::Darken,
            "lighten" => Blend::Lighten,
            "color-dodge" => Blend::ColorDodge,
            "color-burn" => Blend::ColorBurn,
            "hard-light" => Blend::HardLight,
            "soft-light" => Blend::SoftLight,
            "difference" => Blend::Difference,
            "exclusion" => Blend::Exclusion,
            _ => Blend::Normal,
        }
    }

    /// Nom CSS du mode (`mix-blend-mode`), pour HTML et SVG.
    pub fn css_name(&self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Multiply => "multiply",
            Blend::Screen => "screen",
            Blend::Overlay => "overlay",
            Blend::Darken => "darken",
            Blend::Lighten => "lighten",
            Blend::ColorDodge => "color-dodge",
            Blend::ColorBurn => "color-burn",
            Blend::HardLight => "hard-light",
            Blend::SoftLight => "soft-light",
            Blend::Difference => "difference",
            Blend::Exclusion => "exclusion",
        }
    }

    /// Nom PDF du mode (clé /BM d'un ExtGState).
    pub fn pdf_name(&self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Multiply => "Multiply",
            Blend::Screen => "Screen",
            Blend::Overlay => "Overlay",
            Blend::Darken => "Darken",
            Blend::Lighten => "Lighten",
            Blend::ColorDodge => "ColorDodge",
            Blend::ColorBurn => "ColorBurn",
            Blend::HardLight => "HardLight",
            Blend::SoftLight => "SoftLight",
            Blend::Difference => "Difference",
            Blend::Exclusion => "Exclusion",
        }
    }

    /// B(Cb, Cs) sur des canaux normalisés 0–1.
    fn apply(&self, b: f32, s: f32) -> f32 {
        match self {
            Blend::Normal => s,
            Blend::Multiply => b * s,
            Blend::Screen => b + s - b * s,
            Blend::Overlay => Blend::HardLight.apply(s, b),
            Blend::Darken => b.min(s),
            Blend::Lighten => b.max(s),
            Blend::ColorDodge => {
                if b <= 0.0 { 0.0 } else if s >= 1.0 { 1.0 } else { (b / (1.0 - s)).min(1.0) }
            }
            Blend::ColorBurn => {
                if b >= 1.0 { 1.0 } else if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - b) / s).min(1.0) }
            }
            Blend::HardLight => {
                if s <= 0.5 { Blend::Multiply.apply(b, 2.0 * s) } else { Blend::Screen.apply(b, 2.0 * s - 1.0) }
            }
            Blend::SoftLight => {
                if s <= 0.5 {
                    b - (1.0 - 2.0 * s) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                    b + (2.0 * s - 1.0) * (d - b)
                }
            }
            Blend::Difference => (b - s).abs(),
            Blend::Exclusion => b + s - 2.0 * b * s,
        }
    }
}

/// Compose le calque sur l'image : Cr = (1-αs)·Cb + αs·B(Cb, Cs).
fn compose(base: &mut RgbaImage, overlay: &RgbaImage, blend: Blend) {
    for (p, o) in base.pixels_mut().zip(overlay.pixels()) {
        let a = o[3] as f32 / 255.0;
        if a == 0.0 {
            continue;
        }
        for c in 0..3 {
            let cb = p[c] as f32 / 255.0;
            let cs = o[c] as f32 / 255.0;
            let mixed = (1.0 - a) * cb + a * blend.apply(cb, cs);
            p[c] = (mixed * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Signature invisible ajoutée après le calque visible, quand elle est demandée.
fn add_signature(img: DynamicImage, signature: Option<&str>) -> Result<DynamicImage> {
    match signature {
        Some(text) if !text.trim().is_empty() => crate::watermark_stego::embed(&img, text),
        _ => Ok(img),
    }
}

// ── Image : composition pleine résolution ─────────────────────────────────────

/// Applique le calque sur l'image source et enregistre au format demandé.
pub fn apply_to_image(
    input_path: &str,
    overlay_b64: &str,
    output_path: &str,
    format: &crate::conversion_engine::OutputFormat,
    opts: &crate::conversion_engine::ImageOptions,
    blend: Blend,
    signature: Option<&str>,
) -> Result<()> {
    let ext = std::path::Path::new(input_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let src = if ext == "svg" {
        crate::conversion_engine::svg_to_dynamic_image(input_path)?
    } else {
        image::open(input_path).map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?
    };

    let (w, h) = src.dimensions();
    let overlay = fit_overlay(decode_overlay(overlay_b64)?, w, h);

    let mut base = src.to_rgba8();
    compose(&mut base, &overlay, blend);

    let out = add_signature(DynamicImage::ImageRgba8(base), signature)?;
    crate::conversion_engine::save_image(&out, output_path, format, opts)
}

// ── PDF : tampon du calque sur les pages ──────────────────────────────────────

fn obj_num(o: &Object) -> f32 {
    match o {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r,
        _ => 0.0,
    }
}

/// Attribut de page héritable (`MediaBox`, `CropBox`, `Rotate`), en remontant
/// la chaîne `Parent` et en résolvant une éventuelle référence indirecte.
fn inherited_attr<'a>(doc: &'a Document, mut id: lopdf::ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(obj) = dict.get(key) {
            return doc.dereference(obj).ok().map(|(_, o)| o);
        }
        id = dict.get(b"Parent").and_then(|p| p.as_reference()).ok()?;
    }
    None
}

fn page_box(doc: &Document, id: lopdf::ObjectId, key: &[u8]) -> Option<[f32; 4]> {
    let arr = inherited_attr(doc, id, key)?.as_array().ok()?;
    if arr.len() != 4 {
        return None;
    }
    let v: Vec<f32> = arr
        .iter()
        .map(|o| doc.dereference(o).map(|(_, o)| obj_num(o)).unwrap_or(0.0))
        .collect();
    // Normaliser : certains PDF écrivent les coins dans le désordre.
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

/// Zone visible (CropBox bornée par la MediaBox, comme pdf.js) et rotation
/// d'affichage normalisée à 0/90/180/270.
pub fn page_geometry(doc: &Document, id: lopdf::ObjectId) -> ([f32; 4], i64) {
    let media = page_box(doc, id, b"MediaBox").unwrap_or([0.0, 0.0, 595.276, 841.89]);
    let visible = match page_box(doc, id, b"CropBox") {
        Some(c) => [
            c[0].max(media[0]),
            c[1].max(media[1]),
            c[2].min(media[2]),
            c[3].min(media[3]),
        ],
        None => media,
    };
    let rotate = inherited_attr(doc, id, b"Rotate")
        .map(|o| obj_num(o) as i64)
        .unwrap_or(0)
        .rem_euclid(360)
        / 90
        * 90;
    (visible, rotate)
}

/// Matrice `cm` qui pose l'image unité (calque dessiné dans l'orientation
/// affichée) sur la zone visible, en compensant `/Rotate`.
fn stamp_matrix(b: [f32; 4], rotate: i64) -> [f32; 6] {
    let (x0, y0, w, h) = (b[0], b[1], b[2] - b[0], b[3] - b[1]);
    match rotate {
        90 => [0.0, h, -w, 0.0, x0 + w, y0],
        180 => [-w, 0.0, 0.0, -h, x0 + w, y0 + h],
        270 => [0.0, -h, w, 0.0, x0, y0 + h],
        _ => [w, 0.0, 0.0, h, x0, y0],
    }
}

/// Crée l'XObject image du calque : RGB en FlateDecode + /SMask pour l'alpha.
fn add_overlay_xobject(doc: &mut Document, overlay: &RgbaImage) -> Result<lopdf::ObjectId> {
    let (w, h) = (overlay.width(), overlay.height());
    let px = w as usize * h as usize;
    let mut rgb = Vec::with_capacity(px * 3);
    let mut alpha = Vec::with_capacity(px);
    for p in overlay.pixels() {
        rgb.extend_from_slice(&[p[0], p[1], p[2]]);
        alpha.push(p[3]);
    }

    let mut smask = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        alpha,
    );
    smask
        .compress()
        .map_err(|e| anyhow!("Compression masque filigrane: {e}"))?;
    let smask_id = doc.add_object(smask);

    let mut img = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "SMask" => Object::Reference(smask_id),
        },
        rgb,
    );
    img.compress()
        .map_err(|e| anyhow!("Compression filigrane: {e}"))?;
    Ok(doc.add_object(img))
}

/// Nom d'XObject libre dans les ressources d'une page (évite d'écraser l'existant).
fn free_xobject_name(existing: &lopdf::Dictionary) -> Vec<u8> {
    for i in 0..1000 {
        let name = format!("UCWm{i}");
        if !existing.has(name.as_bytes()) {
            return name.into_bytes();
        }
    }
    b"UCWmX".to_vec()
}

/// Un calque et les pages PDF (1-based) sur lesquelles le poser.
pub struct Layer {
    pub overlay: String,
    pub pages: Vec<u32>,
}

/// Pose chaque calque sur ses pages. Une liste de pages vide = toutes les pages.
///
/// Le calque est dessiné sur la zone visible de la page, dans l'orientation
/// affichée (`/Rotate` compensé). Le studio envoie un calque par format de page,
/// sans quoi une page paysage dans un document portrait serait déformée.
/// Le contenu d'origine est encadré par `q`/`Q` pour qu'un état graphique non
/// refermé ne déplace pas le tampon.
pub fn apply_to_pdf(input_path: &str, layers: &[Layer], blend: Blend, output_path: &str) -> Result<()> {
    let mut doc =
        Document::load(input_path).map_err(|e| anyhow!("Ouverture PDF '{}': {}", input_path, e))?;
    let all_pages = doc.get_pages();
    let mut stamped = 0usize;

    for layer in layers {
        let overlay = decode_overlay(&layer.overlay)?;
        let xobj_id = add_overlay_xobject(&mut doc, &overlay)?;
        let targets: Vec<lopdf::ObjectId> = all_pages
            .iter()
            .filter(|(n, _)| layer.pages.is_empty() || layer.pages.contains(n))
            .map(|(_, id)| *id)
            .collect();

        for page_id in targets {
            let (visible, rotate) = page_geometry(&doc, page_id);
            if visible[2] - visible[0] < 1.0 || visible[3] - visible[1] < 1.0 {
                continue;
            }
            let name = register_xobject(&mut doc, page_id, xobj_id)?;
            let gs = register_blend_state(&mut doc, page_id, blend)?;
            let m = stamp_matrix(visible, rotate);
            let draw = format!(
                "Q\nq\n/{} gs\n{} {} {} {} {} {} cm\n/{} Do\nQ\n",
                String::from_utf8_lossy(&gs),
                m[0], m[1], m[2], m[3], m[4], m[5],
                String::from_utf8_lossy(&name)
            );
            let pre_id = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
            let post_id = doc.add_object(Stream::new(dictionary! {}, draw.into_bytes()));

            let page = doc
                .get_object_mut(page_id)
                .and_then(|o| o.as_dict_mut())
                .map_err(|e| anyhow!("Page illisible: {e}"))?;
            let mut contents = match page.get(b"Contents") {
                Ok(Object::Array(a)) => a.clone(),
                Ok(other) => vec![other.clone()],
                Err(_) => Vec::new(),
            };
            contents.insert(0, Object::Reference(pre_id));
            contents.push(Object::Reference(post_id));
            page.set("Contents", Object::Array(contents));
            stamped += 1;
        }
    }

    if stamped == 0 {
        return Err(anyhow!("Aucune page ciblée par le filigrane"));
    }
    doc.save(output_path)
        .map_err(|e| anyhow!("Sauvegarde '{}': {}", output_path, e))?;
    Ok(())
}

/// Déclare l'état graphique du mode de fusion (/ExtGState /BM) et renvoie son nom.
fn register_blend_state(doc: &mut Document, page_id: lopdf::ObjectId, blend: Blend) -> Result<Vec<u8>> {
    let gs_id = doc.add_object(dictionary! {
        "Type" => "ExtGState",
        "BM" => Object::Name(blend.pdf_name().as_bytes().to_vec()),
    });
    let res_id = page_resources_id(doc, page_id)?;
    let inner = doc
        .get_dictionary(res_id)
        .ok()
        .and_then(|r| r.get(b"ExtGState").ok())
        .and_then(|o| o.as_reference().ok());
    let states = match inner {
        Some(id) => doc.get_object_mut(id).and_then(|o| o.as_dict_mut()),
        None => {
            let res = doc
                .get_object_mut(res_id)
                .and_then(|o| o.as_dict_mut())
                .map_err(|e| anyhow!("Ressources page illisibles: {e}"))?;
            if !res.has(b"ExtGState") {
                res.set("ExtGState", lopdf::Dictionary::new());
            }
            res.get_mut(b"ExtGState").and_then(|o| o.as_dict_mut())
        }
    }
    .map_err(|e| anyhow!("/ExtGState illisible: {e}"))?;
    let mut n = 0;
    let mut name = format!("UCGs{n}");
    while states.has(name.as_bytes()) {
        n += 1;
        name = format!("UCGs{n}");
    }
    states.set(name.clone(), Object::Reference(gs_id));
    Ok(name.into_bytes())
}

/// Déclare l'XObject dans les ressources de la page et renvoie son nom.
/// Réutilise le nom si l'XObject y figure déjà (ressources partagées entre pages).
fn register_xobject(
    doc: &mut Document,
    page_id: lopdf::ObjectId,
    xobj_id: lopdf::ObjectId,
) -> Result<Vec<u8>> {
    let res_id = page_resources_id(doc, page_id)?;
    // /XObject peut être en ligne ou indirect : on mute l'objet qui le porte.
    let xs_ref = doc
        .get_dictionary(res_id)
        .ok()
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|o| o.as_reference().ok());
    let xs = match xs_ref {
        Some(id) => doc.get_object_mut(id).and_then(|o| o.as_dict_mut()),
        None => {
            let res = doc
                .get_object_mut(res_id)
                .and_then(|o| o.as_dict_mut())
                .map_err(|e| anyhow!("Ressources page illisibles: {e}"))?;
            if !res.has(b"XObject") {
                res.set("XObject", lopdf::Dictionary::new());
            }
            res.get_mut(b"XObject").and_then(|o| o.as_dict_mut())
        }
    }
    .map_err(|e| anyhow!("/XObject illisible: {e}"))?;

    if let Some((k, _)) = xs
        .iter()
        .find(|(_, v)| v.as_reference().ok() == Some(xobj_id))
    {
        return Ok(k.clone());
    }
    let name = free_xobject_name(xs);
    xs.set(name.clone(), Object::Reference(xobj_id));
    Ok(name)
}

/// ObjectId du dictionnaire /Resources d'une page, en le matérialisant si la
/// page l'hérite de son parent ou ne l'a pas du tout.
fn page_resources_id(doc: &mut Document, page_id: lopdf::ObjectId) -> Result<lopdf::ObjectId> {
    let existing = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"Resources").ok())
        .cloned();

    match existing {
        // Déjà un objet indirect : mutable tel quel.
        Some(Object::Reference(id)) => Ok(id),
        // Dictionnaire en ligne : le sortir en objet indirect pour pouvoir le muter.
        Some(Object::Dictionary(d)) => {
            let id = doc.add_object(Object::Dictionary(d));
            set_page_resources(doc, page_id, id)?;
            Ok(id)
        }
        // Hérité du parent (ou absent) : recopier ce qui est visible.
        _ => {
            let inherited = inherited_resources(doc, page_id).unwrap_or_default();
            let id = doc.add_object(Object::Dictionary(inherited));
            set_page_resources(doc, page_id, id)?;
            Ok(id)
        }
    }
}

fn set_page_resources(
    doc: &mut Document,
    page_id: lopdf::ObjectId,
    res: lopdf::ObjectId,
) -> Result<()> {
    doc.get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(|e| anyhow!("Page illisible: {e}"))?
        .set("Resources", Object::Reference(res));
    Ok(())
}

fn inherited_resources(doc: &Document, mut id: lopdf::ObjectId) -> Option<lopdf::Dictionary> {
    for _ in 0..32 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(res) = dict.get(b"Resources") {
            return match res {
                Object::Dictionary(d) => Some(d.clone()),
                Object::Reference(r) => doc.get_dictionary(*r).ok().cloned(),
                _ => None,
            };
        }
        id = dict.get(b"Parent").and_then(|p| p.as_reference()).ok()?;
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Calque RGBA encodé en PNG base64, moitié gauche rouge opaque.
    fn overlay_b64(w: u32, h: u32) -> String {
        use base64::Engine;
        let mut img = RgbaImage::new(w, h);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            *p = if x < w / 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            };
        }
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&buf)
        )
    }

    /// Le calque doit couvrir la source là où il est opaque et la laisser
    /// intacte là où il est transparent, y compris quand ses dimensions
    /// diffèrent de celles de la source (mise à l'échelle).
    #[test]
    fn calque_composite_et_mis_a_l_echelle() {
        let dir = std::env::temp_dir();
        let src = dir.join("uc_wm_src.png");
        let out = dir.join("uc_wm_out.png");
        image::RgbImage::from_pixel(200, 100, image::Rgb([0, 0, 255]))
            .save(&src)
            .unwrap();

        apply_to_image(
            src.to_str().unwrap(),
            &overlay_b64(50, 25), // volontairement plus petit que la source
            out.to_str().unwrap(),
            &crate::conversion_engine::OutputFormat::Png,
            &crate::conversion_engine::ImageOptions::default(),
            Blend::Normal,
            None,
        )
        .expect("composition");

        let res = image::open(&out).unwrap().to_rgba8();
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
        assert_eq!(res.dimensions(), (200, 100), "sortie à la taille source");
        assert_eq!(res.get_pixel(10, 50)[0], 255, "zone opaque non appliquée");
        assert_eq!(res.get_pixel(190, 50)[2], 255, "zone transparente écrasée");
    }

    /// `/Rotate` : l'image unité doit couvrir exactement la zone visible.
    #[test]
    fn matrice_couvre_la_zone_pour_chaque_rotation() {
        let b = [10.0, 20.0, 110.0, 220.0];
        for r in [0, 90, 180, 270] {
            let m = stamp_matrix(b, r);
            let pts: Vec<(f32, f32)> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                .iter()
                .map(|(i, j)| (m[0] * i + m[2] * j + m[4], m[1] * i + m[3] * j + m[5]))
                .collect();
            let xs: Vec<f32> = pts.iter().map(|p| p.0).collect();
            let ys: Vec<f32> = pts.iter().map(|p| p.1).collect();
            let min = |v: &[f32]| v.iter().cloned().fold(f32::MAX, f32::min);
            let max = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max);
            assert_eq!((min(&xs), max(&xs), min(&ys), max(&ys)), (10.0, 110.0, 20.0, 220.0), "rotation {r}");
        }
        // 90 : le haut-gauche du calque (0,1) tombe sur le bas-gauche non tourné.
        let m = stamp_matrix(b, 90);
        assert_eq!((m[2] + m[4], m[3] + m[5]), (10.0, 20.0));
    }

/// Les modes de fusion doivent suivre les formules W3C (mêmes que le canvas
    /// du navigateur et que PDF), sinon l'aperçu mentirait sur le fichier produit.
    #[test]
    fn formules_de_fusion_conformes() {
        let cas = [
            (Blend::Multiply, 0.5, 0.5, 0.25),
            (Blend::Screen, 0.5, 0.5, 0.75),
            (Blend::Overlay, 0.25, 0.5, 0.25),
            (Blend::HardLight, 0.5, 0.25, 0.25),
            (Blend::Difference, 0.8, 0.3, 0.5),
            (Blend::Exclusion, 0.5, 0.5, 0.5),
            (Blend::ColorBurn, 0.5, 0.5, 0.0),
            (Blend::ColorDodge, 0.5, 0.5, 1.0),
            (Blend::Darken, 0.2, 0.7, 0.2),
            (Blend::Lighten, 0.2, 0.7, 0.7),
        ];
        for (mode, b, s, attendu) in cas {
            let got = mode.apply(b, s);
            assert!((got - attendu).abs() < 1e-3, "{mode:?}({b},{s}) = {got}, attendu {attendu}");
        }
    }

    /// Un mode de fusion doit VRAIMENT altérer les pixels sous le filigrane,
    /// pas seulement les recouvrir en transparence.
    #[test]
    fn fusion_altere_les_pixels_sous_le_filigrane() {
        let dir = std::env::temp_dir();
        let src = dir.join("uc_blend_src.png");
        let (a, b) = (dir.join("uc_blend_normal.png"), dir.join("uc_blend_diff.png"));
        image::RgbImage::from_pixel(120, 60, image::Rgb([200, 120, 60])).save(&src).unwrap();
        let overlay = overlay_b64(120, 60);
        for (out, mode) in [(&a, Blend::Normal), (&b, Blend::Difference)] {
            apply_to_image(
                src.to_str().unwrap(),
                &overlay,
                out.to_str().unwrap(),
                &crate::conversion_engine::OutputFormat::Png,
                &crate::conversion_engine::ImageOptions::default(),
                mode,
                None,
            )
            .expect("composition");
        }
        let (ia, ib) = (image::open(&a).unwrap().to_rgb8(), image::open(&b).unwrap().to_rgb8());
        for f in [&src, &a, &b] {
            let _ = std::fs::remove_file(f);
        }
        // Zone couverte par le calque rouge opaque : les deux modes diffèrent.
        assert_ne!(ia.get_pixel(10, 30), ib.get_pixel(10, 30), "le mode de fusion n'a rien changé");
        // Différence sur du rouge (255,0,0) : |200-255|, |120-0|, |60-0|.
        assert_eq!(ib.get_pixel(10, 30).0, [55, 120, 60]);
        // Hors du calque, l'image reste intacte.
        assert_eq!(ia.get_pixel(110, 30), ib.get_pixel(110, 30));
    }

    /// Signature invisible posée au passage : relisible sur le fichier produit.
    #[test]
    fn signature_invisible_ajoutee_a_la_sortie() {
        let dir = std::env::temp_dir();
        let src = dir.join("uc_sig_src.png");
        let out = dir.join("uc_sig_out.png");
        let mut img = image::RgbImage::new(400, 300);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 200) as u8, (y % 180) as u8, ((x + y) % 255) as u8]);
        }
        img.save(&src).unwrap();
        apply_to_image(
            src.to_str().unwrap(),
            &overlay_b64(400, 300),
            out.to_str().unwrap(),
            &crate::conversion_engine::OutputFormat::Png,
            &crate::conversion_engine::ImageOptions::default(),
            Blend::Normal,
            Some("© Momo"),
        )
        .expect("composition");
        let relu = crate::watermark_stego::extract(&image::open(&out).unwrap());
        for f in [&src, &out] {
            let _ = std::fs::remove_file(f);
        }
        assert_eq!(relu.unwrap(), "© Momo");
    }

/// Chaîne complète, comme le studio : mosaïque dense bruitée fusionnée en
    /// « incrustation », signature invisible, export JPEG — puis EFFACEMENT des
    /// zones marquées par un flou, comme le ferait un outil de retouche.
    #[test]
    fn signature_survit_a_l_effacement_de_la_marque_visible() {
        let dir = std::env::temp_dir();
        let (src, out, att) = (dir.join("uc_chain_src.png"), dir.join("uc_chain_out.jpg"), dir.join("uc_chain_att.jpg"));

        // Photo texturée.
        let mut photo = image::RgbImage::new(900, 600);
        for (x, y, p) in photo.enumerate_pixels_mut() {
            let n = ((x * 7 + y * 13) % 41) as u8;
            *p = image::Rgb([(30 + x % 200) as u8, (40 + y % 180) as u8, (80 + n) as u8]);
        }
        photo.save(&src).unwrap();

        // Calque : mosaïque dense et bruitée (~25 % de la surface).
        let mut layer = RgbaImage::new(900, 600);
        let mut seed = 12345u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 100) as i32
        };
        for (x, y, p) in layer.enumerate_pixels_mut() {
            let (mx, my) = (x % 120, y % 90);
            if mx < 70 && my < 26 {
                let n = rnd() - 50;
                *p = image::Rgba([
                    (220 + n).clamp(0, 255) as u8,
                    (220 + n).clamp(0, 255) as u8,
                    (220 + n).clamp(0, 255) as u8,
                    (150 + n / 2).clamp(0, 255) as u8,
                ]);
            }
        }
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(layer)
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        use base64::Engine;
        let overlay = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&buf)
        );

        apply_to_image(
            src.to_str().unwrap(),
            &overlay,
            out.to_str().unwrap(),
            &crate::conversion_engine::OutputFormat::Jpeg,
            &crate::conversion_engine::ImageOptions { quality: Some(92), ..Default::default() },
            Blend::Overlay,
            Some("© Momo — preuve 2026"),
        )
        .expect("chaîne complète");

        let marked = image::open(&out).unwrap();
        assert_eq!(
            crate::watermark_stego::extract(&marked).unwrap(),
            "© Momo — preuve 2026",
            "signature illisible sur le fichier produit"
        );

        // Effacement : les zones marquées (élargies) sont remplacées par un flou.
        let blurred = marked.blur(10.0).to_rgb8();
        let mut attacked = marked.to_rgb8();
        let mut efface = 0u32;
        for (x, y, p) in attacked.enumerate_pixels_mut() {
            let (mx, my) = (x % 120, y % 90);
            if mx < 78 && my < 34 {
                *p = *blurred.get_pixel(x, y);
                efface += 1;
            }
        }
        let part = efface as f32 / (900.0 * 600.0);
        let mut f = std::fs::File::create(&att).unwrap();
        DynamicImage::ImageRgb8(attacked)
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 92))
            .unwrap();
        drop(f);

        let relu = crate::watermark_stego::extract(&image::open(&att).unwrap());
        for x in [&src, &out, &att] {
            let _ = std::fs::remove_file(x);
        }
        assert_eq!(
            relu.unwrap(),
            "© Momo — preuve 2026",
            "signature perdue après effacement de {:.0} % de l'image",
            part * 100.0
        );
    }

    /// Le tampon PDF doit déclarer le mode de fusion (/ExtGState /BM).
    #[test]
    fn tampon_pdf_conserve_les_pages() {

        let dir = std::env::temp_dir();
        let png = dir.join("uc_wm_pdf_src.png");
        let pdf = dir.join("uc_wm_src.pdf");
        let out = dir.join("uc_wm_stamped.pdf");
        image::RgbImage::from_pixel(64, 32, image::Rgb([10, 200, 10]))
            .save(&png)
            .unwrap();
        let p = png.to_str().unwrap().to_string();
        crate::pdf_engine::images_to_pdf(&[p.clone(), p], pdf.to_str().unwrap()).unwrap();

        let layers = [
            Layer { overlay: overlay_b64(80, 120), pages: vec![1] },
            Layer { overlay: overlay_b64(40, 60), pages: vec![2] },
        ];
        apply_to_pdf(pdf.to_str().unwrap(), &layers, Blend::Multiply, out.to_str().unwrap())
            .expect("tampon PDF");

        let doc = Document::load(&out).expect("PDF relisible");
        let brut = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
        assert!(brut.contains("/BM/Multiply") || brut.contains("/BM /Multiply"), "mode de fusion absent du PDF");
        let pages = doc.get_pages();
        for (n, id) in &pages {
            let content = doc.get_page_content(*id).expect("contenu page");
            let text = String::from_utf8_lossy(&content);
            assert!(text.contains(" Do"), "page {n} sans tampon: {text}");
            assert!(text.contains(" gs"), "page {n} sans état de fusion: {text}");
        }
        let pages = pages.len();
        for f in [&png, &pdf, &out] {
            let _ = std::fs::remove_file(f);
        }
        assert_eq!(pages, 2, "pages perdues par le tampon");
    }
}
