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

// ── Image : composition pleine résolution ─────────────────────────────────────

/// Applique le calque sur l'image source et enregistre au format demandé.
pub fn apply_to_image(
    input_path: &str,
    overlay_b64: &str,
    output_path: &str,
    format: &crate::conversion_engine::OutputFormat,
    opts: &crate::conversion_engine::ImageOptions,
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
    image::imageops::overlay(&mut base, &overlay, 0, 0);

    crate::conversion_engine::save_image(
        &DynamicImage::ImageRgba8(base),
        output_path,
        format,
        opts,
    )
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
pub fn apply_to_pdf(input_path: &str, layers: &[Layer], output_path: &str) -> Result<()> {
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
            let m = stamp_matrix(visible, rotate);
            let draw = format!(
                "Q\nq\n{} {} {} {} {} {} cm\n/{} Do\nQ\n",
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

    /// Le tampon PDF doit produire un fichier relisible, sans perdre de page.
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
        apply_to_pdf(pdf.to_str().unwrap(), &layers, out.to_str().unwrap())
            .expect("tampon PDF");

        let doc = Document::load(&out).expect("PDF relisible");
        let pages = doc.get_pages();
        for (n, id) in &pages {
            let content = doc.get_page_content(*id).expect("contenu page");
            let text = String::from_utf8_lossy(&content);
            assert!(text.contains(" Do"), "page {n} sans tampon: {text}");
        }
        let pages = pages.len();
        for f in [&png, &pdf, &out] {
            let _ = std::fs::remove_file(f);
        }
        assert_eq!(pages, 2, "pages perdues par le tampon");
    }
}
