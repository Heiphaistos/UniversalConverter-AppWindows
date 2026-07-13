/// audio_engine.rs — Conversion audio : MP3 / OGG / FLAC / WAV / M4A / AAC → WAV / FLAC.
/// Décodage pur Rust via symphonia, encodage WAV via hound, FLAC via flacenc.
/// Code partagé desktop / web.

use anyhow::{anyhow, Result};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Plafond d'échantillons décodés (i16) — ~200 MB de PCM, soit ~19 min stéréo 44.1 kHz.
const MAX_SAMPLES: usize = 100_000_000;

pub struct DecodedAudio {
    /// PCM 16 bits entrelacé.
    pub samples: Vec<i16>,
    pub channels: u16,
    pub sample_rate: u32,
}

// ─── Décodage ─────────────────────────────────────────────────────────────────

pub fn decode_audio(input_path: &str, ext: &str) -> Result<DecodedAudio> {
    let file = std::fs::File::open(input_path)
        .map_err(|e| anyhow!("Ouverture '{}': {}", input_path, e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if !ext.is_empty() {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("Format audio non reconnu: {}", e))?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("Aucune piste audio trouvée"))?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("Codec non supporté: {}", e))?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2) as u16;

    let mut samples: Vec<i16> = Vec::new();
    let mut sbuf: Option<SampleBuffer<i16>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(anyhow!("Lecture flux audio: {}", e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                if sbuf.is_none() {
                    let spec = *decoded.spec();
                    sample_rate = spec.rate;
                    channels = spec.channels.count() as u16;
                    sbuf = Some(SampleBuffer::<i16>::new(decoded.capacity() as u64, spec));
                }
                if let Some(buf) = sbuf.as_mut() {
                    buf.copy_interleaved_ref(decoded);
                    if samples.len() + buf.samples().len() > MAX_SAMPLES {
                        return Err(anyhow!(
                            "Audio trop long (limite ~19 min stéréo 44.1 kHz) — conversion refusée"
                        ));
                    }
                    samples.extend_from_slice(buf.samples());
                }
            }
            // Paquet corrompu isolé : on continue
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(anyhow!("Décodage audio: {}", e)),
        }
    }

    if samples.is_empty() {
        return Err(anyhow!("Aucun échantillon audio décodé"));
    }
    Ok(DecodedAudio { samples, channels: channels.max(1), sample_rate })
}

// ─── Encodage WAV ─────────────────────────────────────────────────────────────

pub fn write_wav(audio: &DecodedAudio, output_path: &str) -> Result<()> {
    let spec = hound::WavSpec {
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output_path, spec)
        .map_err(|e| anyhow!("Création WAV '{}': {}", output_path, e))?;
    {
        let mut w16 = writer.get_i16_writer(audio.samples.len() as u32);
        for &s in &audio.samples {
            w16.write_sample(s);
        }
        w16.flush().map_err(|e| anyhow!("WAV write: {}", e))?;
    }
    writer.finalize().map_err(|e| anyhow!("WAV finalize: {}", e))?;
    Ok(())
}

// ─── Encodage FLAC ────────────────────────────────────────────────────────────

pub fn write_flac(audio: &DecodedAudio, output_path: &str) -> Result<()> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    let samples_i32: Vec<i32> = audio.samples.iter().map(|&s| s as i32).collect();
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| anyhow!("Config FLAC invalide: {:?}", e))?;
    let source = flacenc::source::MemSource::from_samples(
        &samples_i32,
        audio.channels as usize,
        16,
        audio.sample_rate as usize,
    );
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| anyhow!("Encodage FLAC: {:?}", e))?;

    let mut sink = ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| anyhow!("FLAC write: {:?}", e))?;
    std::fs::write(output_path, sink.as_slice())
        .map_err(|e| anyhow!("Écriture '{}': {}", output_path, e))
}

// ─── Conversion ───────────────────────────────────────────────────────────────

pub fn convert_audio(input_path: &str, in_ext: &str, output_path: &str, fmt: &str) -> Result<()> {
    let audio = decode_audio(input_path, in_ext)?;
    match fmt {
        "wav" => write_wav(&audio, output_path),
        "flac" => write_flac(&audio, output_path),
        _ => Err(anyhow!("Format audio de sortie non supporté: {}", fmt)),
    }
}
