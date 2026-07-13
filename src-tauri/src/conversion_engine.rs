use anyhow::{anyhow, Result};
use image::ImageFormat;
use std::path::Path;

// ── Formats image supportés ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Gif,
    Tiff,
    Ico,
    Tga,
    Avif,
    Qoi,
    Exr,
    Ppm,
    Ff,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "png"        => Ok(OutputFormat::Png),
            "jpg"|"jpeg" => Ok(OutputFormat::Jpeg),
            "webp"       => Ok(OutputFormat::Webp),
            "bmp"        => Ok(OutputFormat::Bmp),
            "gif"        => Ok(OutputFormat::Gif),
            "tiff"|"tif" => Ok(OutputFormat::Tiff),
            "ico"        => Ok(OutputFormat::Ico),
            "tga"        => Ok(OutputFormat::Tga),
            "avif"       => Ok(OutputFormat::Avif),
            "qoi"        => Ok(OutputFormat::Qoi),
            "exr"        => Ok(OutputFormat::Exr),
            "ppm"|"pnm"  => Ok(OutputFormat::Ppm),
            "ff"         => Ok(OutputFormat::Ff),
            _ => Err(anyhow!("Format image non supporté: {}", s)),
        }
    }

    pub fn to_image_format(&self) -> ImageFormat {
        match self {
            OutputFormat::Png  => ImageFormat::Png,
            OutputFormat::Jpeg => ImageFormat::Jpeg,
            OutputFormat::Webp => ImageFormat::WebP,
            OutputFormat::Bmp  => ImageFormat::Bmp,
            OutputFormat::Gif  => ImageFormat::Gif,
            OutputFormat::Tiff => ImageFormat::Tiff,
            OutputFormat::Ico  => ImageFormat::Ico,
            OutputFormat::Tga  => ImageFormat::Tga,
            OutputFormat::Avif => ImageFormat::Avif,
            OutputFormat::Qoi  => ImageFormat::Qoi,
            OutputFormat::Exr  => ImageFormat::OpenExr,
            OutputFormat::Ppm  => ImageFormat::Pnm,
            OutputFormat::Ff   => ImageFormat::Farbfeld,
        }
    }

    #[allow(dead_code)]
    pub fn extension(&self) -> &str {
        match self {
            OutputFormat::Png  => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Webp => "webp",
            OutputFormat::Bmp  => "bmp",
            OutputFormat::Gif  => "gif",
            OutputFormat::Tiff => "tiff",
            OutputFormat::Ico  => "ico",
            OutputFormat::Tga  => "tga",
            OutputFormat::Avif => "avif",
            OutputFormat::Qoi  => "qoi",
            OutputFormat::Exr  => "exr",
            OutputFormat::Ppm  => "ppm",
            OutputFormat::Ff   => "ff",
        }
    }
}

// ── Options de transformation image ───────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ImageOptions {
    pub quality: Option<u8>,       // 1–100, JPEG seulement
    pub resize_width: Option<u32>,
    pub resize_height: Option<u32>,
    pub rotation: Option<u32>,     // 0, 90, 180, 270
}

fn apply_transforms(img: image::DynamicImage, opts: &ImageOptions) -> image::DynamicImage {
    // 1. Rotation
    let img = match opts.rotation.unwrap_or(0) {
        90  => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _   => img,
    };
    // 2. Redimensionnement (conserve le ratio si une seule dimension)
    match (opts.resize_width, opts.resize_height) {
        (Some(w), Some(h)) => {
            img.resize_exact(w.max(1), h.max(1), image::imageops::FilterType::Lanczos3)
        }
        (Some(w), None) => {
            let src_w = img.width().max(1) as f64;
            let h = ((img.height() as f64 * w as f64) / src_w).round() as u32;
            img.resize_exact(w.max(1), h.max(1), image::imageops::FilterType::Lanczos3)
        }
        (None, Some(h)) => {
            let src_h = img.height().max(1) as f64;
            let w = ((img.width() as f64 * h as f64) / src_h).round() as u32;
            img.resize_exact(w.max(1), h.max(1), image::imageops::FilterType::Lanczos3)
        }
        (None, None) => img,
    }
}

fn save_as_jpeg(img: &image::DynamicImage, output_path: &str, quality: u8) -> Result<()> {
    use image::codecs::jpeg::JpegEncoder;
    let file = std::fs::File::create(output_path)
        .map_err(|e| anyhow!("Création '{}': {}", output_path, e))?;
    let mut writer = std::io::BufWriter::new(file);
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    JpegEncoder::new_with_quality(&mut writer, quality)
        .encode_image(&rgb)
        .map_err(|e| anyhow!("JPEG encode: {}", e))
}

// ── Formats disponibles selon l'extension d'entrée ────────────────────────────

pub fn get_available_formats(input_ext: &str) -> Vec<&'static str> {
    match input_ext.to_lowercase().as_str() {
        // Images raster (avif : sortie uniquement — décodage non supporté sans lib C)
        "png"|"jpg"|"jpeg"|"webp"|"bmp"|"gif"|"tiff"|"tif"|"tga"|"pnm"|"ppm"|"hdr"|"ico"|"qoi"|"exr"|"dds"|"ff" => {
            vec!["png", "jpg", "webp", "bmp", "gif", "tiff", "tga", "ico", "avif", "qoi", "exr", "ppm", "ff", "pdf"]
        }
        "svg"  => vec!["png", "jpg", "webp", "bmp", "avif", "qoi", "pdf"],
        // Documents
        "pdf"  => vec!["txt", "html", "md", "docx", "epub", "rtf", "odt"],
        "txt"  => vec!["pdf", "html", "md", "docx", "epub", "rtf", "odt"],
        "md" | "markdown" => vec!["html", "txt", "pdf", "docx", "epub", "rtf", "odt"],
        "html" | "htm"    => vec!["txt", "md", "pdf", "docx", "epub", "rtf", "odt"],
        "docx" | "doc"    => vec!["txt", "html", "md", "pdf", "epub", "rtf", "odt"],
        "pptx" | "ppt" | "odp" => vec!["txt", "md", "html", "pdf", "docx", "epub", "rtf", "odt"],
        "rtf"  => vec!["txt", "html", "md", "pdf", "docx", "epub", "odt"],
        "epub" => vec!["txt", "html", "md", "pdf", "docx", "rtf", "odt"],
        "odt"  => vec!["txt", "html", "md", "pdf", "docx", "epub", "rtf"],
        // Tableurs & data
        "xlsx" => vec!["csv", "json", "yaml", "toml", "xml", "txt", "html", "md", "pdf"],
        "xls" | "ods" => vec!["csv", "json", "yaml", "toml", "xml", "xlsx", "txt", "html", "md", "pdf"],
        "csv"  => vec!["json", "yaml", "toml", "xml", "xlsx", "txt", "html", "md", "pdf"],
        "json" => vec!["csv", "yaml", "toml", "xml", "xlsx", "txt", "pdf"],
        "yaml" | "yml" => vec!["json", "toml", "xml", "csv", "xlsx", "txt", "pdf"],
        "toml" => vec!["json", "yaml", "xml", "txt", "pdf"],
        "xml"  => vec!["json", "yaml", "toml", "txt", "pdf"],
        // Sous-titres
        "srt"  => vec!["vtt", "txt"],
        "vtt"  => vec!["srt", "txt"],
        // Archives
        "zip"  => vec!["tar", "tgz"],
        "tar"  => vec!["zip", "tgz"],
        "tgz" | "gz" => vec!["zip", "tar"],
        "7z"   => vec!["zip", "tar", "tgz"],
        // Audio
        "mp3" | "ogg" | "m4a" | "aac" => vec!["wav", "flac"],
        "wav"  => vec!["flac"],
        "flac" => vec!["wav"],
        _ => vec![],
    }
}

// ── Conversion image raster → image ───────────────────────────────────────────

pub fn convert_image_file(
    input_path: &str,
    output_path: &str,
    format: &OutputFormat,
    opts: &ImageOptions,
) -> Result<()> {
    let img = image::open(input_path)
        .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;

    let img = apply_transforms(img, opts);

    let img = if matches!(format, OutputFormat::Ico) {
        img.thumbnail(256, 256)
    } else {
        img
    };

    save_image(&img, output_path, format, opts)
}

/// Sauvegarde avec adaptation du type de pixels aux exigences de l'encodeur.
fn save_image(
    img: &image::DynamicImage,
    output_path: &str,
    format: &OutputFormat,
    opts: &ImageOptions,
) -> Result<()> {
    match format {
        OutputFormat::Jpeg => {
            let q = opts.quality.unwrap_or(90).clamp(1, 100);
            save_as_jpeg(img, output_path, q)
        }
        // EXR : encodeur float uniquement
        OutputFormat::Exr => {
            image::DynamicImage::ImageRgba32F(img.to_rgba32f())
                .save_with_format(output_path, ImageFormat::OpenExr)
                .map_err(|e| anyhow!("Sauvegarde '{}': {}", output_path, e))
        }
        // Farbfeld : RGBA 16 bits uniquement
        OutputFormat::Ff => {
            image::DynamicImage::ImageRgba16(img.to_rgba16())
                .save_with_format(output_path, ImageFormat::Farbfeld)
                .map_err(|e| anyhow!("Sauvegarde '{}': {}", output_path, e))
        }
        // PPM : RGB 8 bits (pas d'alpha)
        OutputFormat::Ppm => {
            image::DynamicImage::ImageRgb8(img.to_rgb8())
                .save_with_format(output_path, ImageFormat::Pnm)
                .map_err(|e| anyhow!("Sauvegarde '{}': {}", output_path, e))
        }
        // AVIF / QOI : RGBA 8 bits
        OutputFormat::Avif | OutputFormat::Qoi => {
            image::DynamicImage::ImageRgba8(img.to_rgba8())
                .save_with_format(output_path, format.to_image_format())
                .map_err(|e| anyhow!("Sauvegarde '{}': {}", output_path, e))
        }
        _ => img
            .save_with_format(output_path, format.to_image_format())
            .map_err(|e| anyhow!("Sauvegarde '{}': {}", output_path, e)),
    }
}

// ── Conversion SVG → image raster ─────────────────────────────────────────────

pub fn convert_svg_to_image(
    input_path: &str,
    output_path: &str,
    format: &OutputFormat,
    opts: &ImageOptions,
) -> Result<()> {
    let img = svg_to_dynamic_image(input_path)?;
    let img = apply_transforms(img, opts);
    save_image(&img, output_path, format, opts)
}

// ── Rendu SVG → DynamicImage ──────────────────────────────────────────────────

pub fn svg_to_dynamic_image(svg_path: &str) -> Result<image::DynamicImage> {
    use resvg::{tiny_skia, usvg};

    let svg_data = std::fs::read(svg_path)
        .map_err(|e| anyhow!("Lecture '{}': {}", svg_path, e))?;

    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &options)
        .map_err(|e| anyhow!("SVG invalide: {}", e))?;

    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| anyhow!("Dimensions SVG invalides"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    let rgba = image::RgbaImage::from_raw(size.width(), size.height(), pixmap.take())
        .ok_or_else(|| anyhow!("Conversion pixmap échouée"))?;
    Ok(image::DynamicImage::ImageRgba8(rgba))
}

// ── Miniature base64 PNG (120×120 max) ────────────────────────────────────────

pub fn generate_thumbnail(input_path: &str) -> Result<String> {
    use std::io::Cursor;

    let ext = Path::new(input_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let img = if ext == "svg" {
        svg_to_dynamic_image(input_path)?
    } else {
        image::open(input_path)
            .map_err(|e| anyhow!("Thumbnail '{}': {}", input_path, e))?
    };

    let thumb = img.thumbnail(120, 120);
    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| anyhow!("Thumbnail encode: {}", e))?;

    use base64::Engine;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buf)
    ))
}

// ── Génération chemin de sortie ────────────────────────────────────────────────

/// Valide qu'un composant de nom de fichier ne contient pas de séquences dangereuses.
/// Supprime ou remplace tous les caractères interdits sur Windows/POSIX.
fn sanitize_name(name: &str) -> String {
    let s = name
        .replace('\0', "")
        .replace('/', "_")
        .replace('\\', "_")
        .replace(':', "_")
        .replace('*', "_")
        .replace('?', "_")
        .replace('"', "_")
        .replace('<', "_")
        .replace('>', "_")
        .replace('|', "_");

    // Supprimer toute séquence "traversal" (.. ou encodages courants)
    let s = s.replace("..", "");

    // Supprimer les espaces de début/fin et points multiples
    let s = s.trim().trim_matches('.').to_string();

    if s.is_empty() { "file".to_string() } else { s }
}

/// Construit le chemin de sortie avec dossier et nom personnalisés optionnels.
pub fn build_output_path_custom(
    input_path: &str,
    new_ext: &str,
    output_dir: Option<&str>,
    output_name: Option<&str>,
) -> String {
    let stem = match output_name {
        Some(n) if !n.trim().is_empty() => sanitize_name(n),
        _ => {
            let s = Path::new(input_path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            format!("{}_converted", s)
        }
    };
    let dir = match output_dir {
        Some(d) => Path::new(d).to_path_buf(),
        None    => Path::new(input_path).parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    dir.join(format!("{}.{}", stem, new_ext)).to_string_lossy().to_string()
}

