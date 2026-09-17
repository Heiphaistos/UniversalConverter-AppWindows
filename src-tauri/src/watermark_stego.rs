//! Signature invisible : le texte du propriétaire est inscrit dans le domaine
//! fréquentiel de l'image (DCT 8×8 sur la luminance), pas dans les pixels.
//!
//! Méthode Koch-Zhao : dans chaque bloc, on force une relation d'ordre entre deux
//! coefficients de moyenne fréquence. Ces coefficients survivent au réencodage
//! JPEG, au contraire du bit de poids faible d'un pixel. Chaque bit est répété
//! Le message est répété sur TOUTE l'image (des dizaines de copies) et relu par
//! un vote pondéré par la force du signal : effacer, flouter ou repeindre une
//! partie de l'image ne fait perdre qu'une fraction des copies.
//!
//! Limites assumées : un redimensionnement, une rotation ou une recompression
//! très agressive détruisent la signature. Elle prouve l'origine d'un fichier
//! diffusé tel quel ou légèrement recompressé, elle ne résiste pas à tout.

use anyhow::{anyhow, Result};
use image::{DynamicImage, GenericImageView, RgbImage};

/// Écart imposé entre les deux coefficients (en unités DCT).
/// Plus il est grand, plus la signature survit, plus elle marque l'image.
const DELTA: f32 = 26.0;
/// Coefficients de moyenne fréquence comparés (zigzag ~ 12 et 13).
const C1: (usize, usize) = (3, 2);
const C2: (usize, usize) = (2, 3);
/// En-tête : nombre d'octets utiles, sur 16 bits, à répétition fixe (connue des
/// deux côtés, puisqu'il faut le lire avant de savoir combien de copies du
/// message l'image contient).
const HEADER_BITS: usize = 16;
const HEADER_REPEAT: usize = 40;
const HEADER_BLOCKS: usize = HEADER_BITS * HEADER_REPEAT;
/// Copies minimales du message : en dessous, l'image est jugée trop petite.
const MIN_REPEAT: usize = 6;
/// Plafond de copies : au-delà, on n'y gagne plus rien.
const MAX_REPEAT: usize = 96;
const MAX_PAYLOAD: usize = 512;

/// Nombre de copies du message que l'image peut porter, et donc le pas de
/// répétition utilisé à l'écriture comme à la lecture.
fn payload_repeat(total_blocks: usize, payload_bits: usize) -> usize {
    if payload_bits == 0 || total_blocks <= HEADER_BLOCKS {
        return 0;
    }
    ((total_blocks - HEADER_BLOCKS) / payload_bits).min(MAX_REPEAT)
}

// ── DCT 8×8 ───────────────────────────────────────────────────────────────────

fn cos_table() -> [[f32; 8]; 8] {
    let mut t = [[0.0f32; 8]; 8];
    for (x, row) in t.iter_mut().enumerate() {
        for (u, v) in row.iter_mut().enumerate() {
            *v = (((2 * x + 1) as f32) * (u as f32) * std::f32::consts::PI / 16.0).cos();
        }
    }
    t
}

fn alpha(u: usize) -> f32 {
    if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 }
}

fn dct8(block: &[[f32; 8]; 8], t: &[[f32; 8]; 8]) -> [[f32; 8]; 8] {
    let mut out = [[0.0f32; 8]; 8];
    for (u, row) in out.iter_mut().enumerate() {
        for (v, cell) in row.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (x, brow) in block.iter().enumerate() {
                for (y, px) in brow.iter().enumerate() {
                    sum += px * t[x][u] * t[y][v];
                }
            }
            *cell = 0.25 * alpha(u) * alpha(v) * sum;
        }
    }
    out
}

fn idct8(coef: &[[f32; 8]; 8], t: &[[f32; 8]; 8]) -> [[f32; 8]; 8] {
    let mut out = [[0.0f32; 8]; 8];
    for (x, row) in out.iter_mut().enumerate() {
        for (y, cell) in row.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (u, crow) in coef.iter().enumerate() {
                for (v, c) in crow.iter().enumerate() {
                    sum += alpha(u) * alpha(v) * c * t[x][u] * t[y][v];
                }
            }
            *cell = 0.25 * sum;
        }
    }
    out
}

// ── Bits ─────────────────────────────────────────────────────────────────────

fn bits_of(payload: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(HEADER_BITS + payload.len() * 8);
    let len = payload.len() as u16;
    for i in (0..HEADER_BITS).rev() {
        bits.push((len >> i) & 1 == 1);
    }
    for byte in payload {
        for i in (0..8).rev() {
            bits.push((byte >> i) & 1 == 1);
        }
    }
    bits
}

/// Ordre de parcours des blocs : dispersé, pour qu'un rognage ou une retouche
/// locale n'emporte pas tous les exemplaires d'un même bit.
fn block_order(count: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    // Permutation déterministe (générateur congruentiel), même clé des deux côtés.
    let mut state: u64 = 0x5DEECE66D;
    for i in (1..count).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        order.swap(i, j);
    }
    order
}

/// Nombre de caractères que l'image peut porter.
pub fn capacity_bytes(width: u32, height: u32) -> usize {
    let blocks = (width as usize / 8) * (height as usize / 8);
    blocks.saturating_sub(HEADER_BLOCKS) / MIN_REPEAT / 8
}

/// Bit porté par un bloc donné : en-tête au début, puis le message en boucle.
fn bit_at(bits: &[bool], slot: usize, repeat: usize) -> Option<bool> {
    if slot < HEADER_BLOCKS {
        return bits.get(slot / HEADER_REPEAT).copied();
    }
    let payload_bits = bits.len() - HEADER_BITS;
    if payload_bits == 0 || repeat == 0 {
        return None;
    }
    let pos = slot - HEADER_BLOCKS;
    if pos >= payload_bits * repeat {
        return None; // au-delà des copies prévues : bloc laissé intact
    }
    bits.get(HEADER_BITS + pos % payload_bits).copied()
}

// ── Écriture ─────────────────────────────────────────────────────────────────

/// Inscrit `text` dans la luminance de l'image et renvoie l'image modifiée.
pub fn embed(img: &DynamicImage, text: &str) -> Result<DynamicImage> {
    let payload = text.as_bytes();
    if payload.is_empty() {
        return Err(anyhow!("Signature invisible vide"));
    }
    if payload.len() > MAX_PAYLOAD {
        return Err(anyhow!("Signature invisible trop longue ({} octets max)", MAX_PAYLOAD));
    }
    let (w, h) = img.dimensions();
    let capacity = capacity_bytes(w, h);
    if payload.len() > capacity {
        return Err(anyhow!(
            "Image trop petite pour cette signature : {} caractères possibles, {} demandés",
            capacity,
            payload.len()
        ));
    }

    let mut rgb: RgbImage = img.to_rgb8();
    let bits = bits_of(payload);
    let bw = w as usize / 8;
    let bh = h as usize / 8;
    let order = block_order(bw * bh);
    let t = cos_table();

    let repeat = payload_repeat(order.len(), bits.len() - HEADER_BITS);
    for (slot, &block_index) in order.iter().enumerate() {
        // En-tête sur les premiers blocs, puis le message répété sur tout le reste :
        // effacer une zone n'emporte qu'une fraction des copies de chaque bit.
        let Some(bit) = bit_at(&bits, slot, repeat) else { break };
        let (bx, by) = (block_index % bw, block_index / bw);

        // Luminance du bloc.
        let mut block = [[0.0f32; 8]; 8];
        for (y, row) in block.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let p = rgb.get_pixel((bx * 8 + x) as u32, (by * 8 + y) as u32).0;
                *cell = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32 - 128.0;
            }
        }

        let mut coef = dct8(&block, &t);
        let (a, b) = (coef[C1.0][C1.1], coef[C2.0][C2.1]);
        // Bit 1 : |a| doit dépasser |b| de DELTA. Bit 0 : l'inverse.
        let (mut na, mut nb) = (a, b);
        let mid = (a.abs() + b.abs()) / 2.0;
        if bit {
            if a.abs() < b.abs() + DELTA {
                na = (mid + DELTA / 2.0).copysign(if a == 0.0 { 1.0 } else { a });
                nb = (mid - DELTA / 2.0).max(0.0).copysign(if b == 0.0 { 1.0 } else { b });
            }
        } else if b.abs() < a.abs() + DELTA {
            nb = (mid + DELTA / 2.0).copysign(if b == 0.0 { 1.0 } else { b });
            na = (mid - DELTA / 2.0).max(0.0).copysign(if a == 0.0 { 1.0 } else { a });
        }
        if na == a && nb == b {
            continue; // le bloc porte déjà le bon bit
        }
        coef[C1.0][C1.1] = na;
        coef[C2.0][C2.1] = nb;

        // Retour aux pixels : on applique l'écart de luminance en gardant la couleur.
        let restored = idct8(&coef, &t);
        for (y, row) in restored.iter().enumerate() {
            for (x, lum) in row.iter().enumerate() {
                let (px, py) = ((bx * 8 + x) as u32, (by * 8 + y) as u32);
                let p = rgb.get_pixel(px, py).0;
                let old = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32 - 128.0;
                let d = lum - old;
                let np = [
                    (p[0] as f32 + d).clamp(0.0, 255.0) as u8,
                    (p[1] as f32 + d).clamp(0.0, 255.0) as u8,
                    (p[2] as f32 + d).clamp(0.0, 255.0) as u8,
                ];
                rgb.put_pixel(px, py, image::Rgb(np));
            }
        }
    }

    Ok(DynamicImage::ImageRgb8(rgb))
}

// ── Lecture ──────────────────────────────────────────────────────────────────

/// Relit la signature d'une image, sans avoir besoin de l'originale.
pub fn extract(img: &DynamicImage) -> Result<String> {
    let (w, h) = img.dimensions();
    if w < 64 || h < 64 {
        return Err(anyhow!("Image trop petite pour porter une signature"));
    }
    let rgb = img.to_rgb8();
    let bw = w as usize / 8;
    let bh = h as usize / 8;
    let order = block_order(bw * bh);
    let t = cos_table();

    // Vote PONDÉRÉ : un bloc dont les deux coefficients sont presque égaux
    // (flou, repeint, aplat) ne porte plus d'information et ne doit pas peser
    // autant qu'un bloc intact. Sans cela, effacer la marque visible suffisait
    // à noyer la signature sous des votes aléatoires.
    let force = |block_index: usize| -> f32 {
        let (bx, by) = (block_index % bw, block_index / bw);
        let mut block = [[0.0f32; 8]; 8];
        for (y, row) in block.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let p = rgb.get_pixel((bx * 8 + x) as u32, (by * 8 + y) as u32).0;
                *cell = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32 - 128.0;
            }
        }
        let coef = dct8(&block, &t);
        ((coef[C1.0][C1.1].abs() - coef[C2.0][C2.1].abs()) / DELTA).clamp(-1.0, 1.0)
    };

    // 1. En-tête : longueur du message.
    let mut header = [0f32; HEADER_BITS];
    for (slot, &block_index) in order.iter().take(HEADER_BLOCKS.min(order.len())).enumerate() {
        header[slot / HEADER_REPEAT] += force(block_index);
    }
    let mut len: usize = 0;
    for v in header {
        len = (len << 1) | usize::from(v > 0.0);
    }
    let payload_bits = len * 8;
    let repeat = payload_repeat(order.len(), payload_bits);
    if len == 0 || len > MAX_PAYLOAD || repeat < MIN_REPEAT {
        return Err(anyhow!("Aucune signature invisible détectée"));
    }

    // 2. Message : chaque bit additionne toutes ses copies réparties dans l'image.
    let mut votes = vec![0f32; payload_bits];
    for (slot, &block_index) in order.iter().enumerate().skip(HEADER_BLOCKS) {
        let pos = slot - HEADER_BLOCKS;
        if pos >= payload_bits * repeat {
            break;
        }
        votes[pos % payload_bits] += force(block_index);
    }

    let mut bytes = Vec::with_capacity(len);
    for i in 0..len {
        let mut byte = 0u8;
        for j in 0..8 {
            byte = (byte << 1) | u8::from(votes[i * 8 + j] > 0.0);
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).map_err(|_| anyhow!("Signature illisible (image trop dégradée)"))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Image « photo » : dégradés + texture, comme une vraie image.
    fn photo(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let n = ((x * 7 + y * 13) % 37) as u8;
            *p = image::Rgb([
                (40 + x % 180) as u8,
                (30 + y % 200).min(255) as u8,
                (90 + n) as u8,
            ]);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn signature_relue_a_l_identique() {
        let img = photo(640, 480);
        let marked = embed(&img, "© Momo 2026 — ForgeInformatique").unwrap();
        assert_eq!(extract(&marked).unwrap(), "© Momo 2026 — ForgeInformatique");
    }

    #[test]
    fn image_sans_signature_ne_ment_pas() {
        // Une image vierge ne doit pas produire de faux texte.
        assert!(extract(&photo(320, 320)).is_err());
    }

    /// La signature doit survivre à un réencodage JPEG de qualité courante.
    #[test]
    fn signature_survit_au_jpeg() {
        let marked = embed(&photo(800, 600), "PROPRIETE MOMO").unwrap();
        let mut buf = Vec::new();
        marked
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85))
            .unwrap();
        let reread = image::load_from_memory(&buf).unwrap();
        assert_eq!(extract(&reread).unwrap(), "PROPRIETE MOMO");
    }

    /// Repeindre une partie de l'image (ce que fait une IA) ne doit pas suffire.
    #[test]
    fn signature_survit_a_un_repeint_partiel() {
        let marked = embed(&photo(800, 600), "MOMO").unwrap();
        let mut rgb = marked.to_rgb8();
        for y in 150..450 {
            for x in 200..600 {
                rgb.put_pixel(x, y, image::Rgb([128, 128, 128])); // ~25 % de la surface effacée
            }
        }
        assert_eq!(extract(&DynamicImage::ImageRgb8(rgb)).unwrap(), "MOMO");
    }

    /// Effacement large (40 % de l'image aplatie, comme un remplissage
    /// automatique sur une mosaïque dense) : la signature doit tenir.
    #[test]
    fn signature_survit_a_un_effacement_large() {
        let marked = embed(&photo(900, 600), "© Momo — preuve").unwrap();
        let mut rgb = marked.to_rgb8();
        for y in 0..600u32 {
            for x in 0..900u32 {
                // Bandes couvrant ~40 % de la surface, remplies d'un aplat.
                if (x / 60 + y / 60) % 5 < 2 {
                    rgb.put_pixel(x, y, image::Rgb([120, 120, 120]));
                }
            }
        }
        assert_eq!(extract(&DynamicImage::ImageRgb8(rgb)).unwrap(), "© Momo — preuve");
    }

    /// Attaque réaliste : la marque visible (mosaïque dense) est effacée par un
    /// flou, puis l'image est recompressée. C'est ce que fait un outil de retouche.
    #[test]
    fn signature_survit_a_un_flou_sur_la_marque() {
        let marked = embed(&photo(900, 600), "© Momo — preuve 2026").unwrap();
        let blurred = marked.blur(12.0).to_rgb8();
        let mut rgb = marked.to_rgb8();
        // ~35 % de la surface remplacée par la version floutée (zones de la mosaïque).
        for y in 0..600u32 {
            for x in 0..900u32 {
                if (x / 45 + y / 45) % 3 == 0 {
                    rgb.put_pixel(x, y, *blurred.get_pixel(x, y));
                }
            }
        }
        let mut buf = Vec::new();
        DynamicImage::ImageRgb8(rgb)
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92))
            .unwrap();
        let relu = extract(&image::load_from_memory(&buf).unwrap());
        assert_eq!(relu.unwrap(), "© Momo — preuve 2026");
    }

    /// La signature reste discrète à l'œil : écart moyen faible.
    #[test]
    fn signature_reste_discrete() {
        let img = photo(640, 480);
        let marked = embed(&img, "© Momo").unwrap();
        let (a, b) = (img.to_rgb8(), marked.to_rgb8());
        let total: f64 = a
            .pixels()
            .zip(b.pixels())
            .map(|(p, q)| (p[0] as f64 - q[0] as f64).abs() + (p[1] as f64 - q[1] as f64).abs() + (p[2] as f64 - q[2] as f64).abs())
            .sum();
        let mean = total / (a.pixels().len() as f64 * 3.0);
        assert!(mean < 3.0, "signature trop visible : écart moyen {mean:.2}/255");
    }

    /// Diagnostic sur un fichier réel : `UC_DIAG_IMAGE=<chemin> cargo test diagnostic -- --nocapture`.
    /// Sans la variable, le test ne fait rien.
    #[test]
    fn diagnostic_fichier_reel() {
        let Ok(path) = std::env::var("UC_DIAG_IMAGE") else { return };
        let img = image::open(&path).expect("image lisible");
        let (w, h) = img.dimensions();
        let rgb = img.to_rgb8();
        let (bw, bh) = (w as usize / 8, h as usize / 8);
        let order = block_order(bw * bh);
        let t = cos_table();
        let mut forces = Vec::new();
        for &bi in order.iter() {
            let (bx, by) = (bi % bw, bi / bw);
            let mut block = [[0.0f32; 8]; 8];
            for (y, row) in block.iter_mut().enumerate() {
                for (x, cell) in row.iter_mut().enumerate() {
                    let p = rgb.get_pixel((bx * 8 + x) as u32, (by * 8 + y) as u32).0;
                    *cell = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32 - 128.0;
                }
            }
            let c = dct8(&block, &t);
            forces.push((c[C1.0][C1.1].abs() - c[C2.0][C2.1].abs()).abs());
        }
        let mut tri = forces.clone();
        tri.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = tri[tri.len() / 2];
        let faibles = forces.iter().filter(|f| **f < DELTA / 3.0).count() as f32 / forces.len() as f32;
        println!("DIAG {path}: {w}x{h}, {} blocs, force médiane {med:.1} (DELTA={DELTA}), blocs faibles {:.0}%", forces.len(), faibles * 100.0);
        println!("DIAG extraction : {:?}", extract(&img));
    }

    #[test]
    fn capacite_annoncee_et_refus_clair() {
        assert!(capacity_bytes(640, 480) > 60);
        let petite = photo(64, 64);
        let err = embed(&petite, &"x".repeat(200)).unwrap_err().to_string();
        assert!(err.contains("trop petite"), "message peu clair : {err}");
    }
}
