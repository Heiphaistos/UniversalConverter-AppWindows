//! Filigrane des formats sans image : sous-titres (cue permanent) et audio
//! (tatouage sonore mixé à intervalle régulier).

use anyhow::{anyhow, Result};
use crate::audio_engine::{decode_audio, write_flac, write_wav, DecodedAudio};

// ── Sous-titres ───────────────────────────────────────────────────────────────

/// Texte de cue sûr : pas de ligne vide (fin de cue) ni de flèche de timing.
fn cue_text(text: &str) -> String {
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.replace("-->", "->").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() { "©".to_string() } else { lines.join("\n") }
}

/// Zone `{\anN}` (pavé numérique) la plus proche d'une position 0–1.
fn srt_alignment(x: f32, y: f32) -> u8 {
    let col = if x < 1.0 / 3.0 { 0 } else if x < 2.0 / 3.0 { 1 } else { 2 };
    let base = if y < 1.0 / 3.0 { 7 } else if y < 2.0 / 3.0 { 4 } else { 1 };
    base + col
}

/// Ajoute un cue couvrant toute la durée, placé selon `x`/`y` (0–1).
pub fn watermark_subtitles(input: &str, ext: &str, text: &str, x: f32, y: f32) -> Result<String> {
    let text = cue_text(text);
    let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));

    match ext {
        "srt" => {
            // Renuméroter : un index est une ligne de chiffres suivie d'un timing.
            let lines: Vec<&str> = input.lines().collect();
            let mut body = String::with_capacity(input.len() + 128);
            for (i, line) in lines.iter().enumerate() {
                let t = line.trim_start_matches('\u{feff}').trim();
                let next_is_timing = lines.get(i + 1).is_some_and(|n| n.contains("-->"));
                match t.parse::<u64>() {
                    Ok(n) if next_is_timing => body.push_str(&(n + 1).to_string()),
                    _ => body.push_str(line.trim_start_matches('\u{feff}')),
                }
                body.push('\n');
            }
            Ok(format!(
                "1\n00:00:00,000 --> 99:59:59,999\n{{\\an{}}}{}\n\n{}",
                srt_alignment(x, y),
                text,
                body
            ))
        }
        "vtt" => {
            let src = input.trim_start_matches('\u{feff}');
            if !src.trim_start().starts_with("WEBVTT") {
                return Err(anyhow!("Fichier WebVTT invalide (en-tête WEBVTT absent)"));
            }
            // Le cue s'insère après le bloc d'en-tête (jusqu'à la première ligne vide).
            let (header, rest) = match src.find("\n\n").or_else(|| src.find("\r\n\r\n")) {
                Some(i) => src.split_at(i),
                None => (src, ""),
            };
            Ok(format!(
                "{}\n\n00:00:00.000 --> 99:59:59.999 line:{:.0}% position:{:.0}% align:center\n{}\n{}\n",
                header.trim_end(),
                y * 100.0,
                x * 100.0,
                text,
                rest
            ))
        }
        _ => Err(anyhow!("Format de sous-titres non supporté : {ext}")),
    }
}

// ── Audio ─────────────────────────────────────────────────────────────────────

/// Réglages du tatouage sonore.
pub struct AudioMark {
    pub interval_s: f32, // 0 = une seule fois
    pub offset_s: f32,
    pub volume: f32,     // 0–2
}

/// Convertit le son vers le nombre de canaux et la fréquence de la piste.
/// ponytail: rééchantillonnage linéaire, suffisant pour un jingle ; passer à
/// un filtre sinc si le son de tatouage doit rester parfaitement propre.
fn conform(sound: &DecodedAudio, channels: u16, rate: u32) -> Vec<i16> {
    let sc = sound.channels.max(1) as usize;
    let frames = sound.samples.len() / sc;
    let mono_or_first = |f: usize, c: usize| -> f32 {
        let c = if c < sc { c } else { 0 };
        sound.samples[f * sc + c] as f32
    };
    let ratio = sound.sample_rate as f64 / rate as f64;
    let out_frames = ((frames as f64) / ratio).floor() as usize;
    let tc = channels.max(1) as usize;
    let mut out = Vec::with_capacity(out_frames * tc);
    for i in 0..out_frames {
        let pos = i as f64 * ratio;
        let f0 = pos.floor() as usize;
        let f1 = (f0 + 1).min(frames.saturating_sub(1));
        let t = (pos - f0 as f64) as f32;
        for c in 0..tc {
            let v = mono_or_first(f0, c) * (1.0 - t) + mono_or_first(f1, c) * t;
            out.push(v as i16);
        }
    }
    out
}

/// Mixe `sound` dans `track` selon `mark`.
pub fn mix(track: &mut DecodedAudio, sound: &DecodedAudio, mark: &AudioMark) -> Result<usize> {
    let tc = track.channels.max(1) as usize;
    let sound = conform(sound, track.channels, track.sample_rate);
    if sound.is_empty() {
        return Err(anyhow!("Son de tatouage vide"));
    }
    let volume = mark.volume.clamp(0.0, 2.0);
    let frame_of = |s: f32| (s.max(0.0) as f64 * track.sample_rate as f64) as usize;
    let total_frames = track.samples.len() / tc;
    let step = if mark.interval_s > 0.0 { frame_of(mark.interval_s).max(1) } else { usize::MAX };

    let mut start = frame_of(mark.offset_s);
    let mut count = 0;
    while start < total_frames {
        let begin = start * tc;
        for (k, &s) in sound.iter().enumerate() {
            let Some(dst) = track.samples.get_mut(begin + k) else { break };
            *dst = (*dst as f32 + s as f32 * volume).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
        count += 1;
        start = match start.checked_add(step) { Some(v) => v, None => break };
    }
    if count == 0 {
        return Err(anyhow!("Le décalage dépasse la durée de la piste"));
    }
    Ok(count)
}

pub fn watermark_audio(
    input: &str,
    in_ext: &str,
    sound_path: &str,
    mark: &AudioMark,
    output: &str,
    fmt: &str,
) -> Result<()> {
    let sound_ext = std::path::Path::new(sound_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mut track = decode_audio(input, in_ext)?;
    let sound = decode_audio(sound_path, &sound_ext)?;
    mix(&mut track, &sound, mark)?;
    match fmt {
        "wav" => write_wav(&track, output),
        "flac" => write_flac(&track, output),
        _ => Err(anyhow!("Format audio de sortie non supporté : {fmt} (wav ou flac)")),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_ajoute_un_cue_et_renumerote() {
        let src = "1\n00:00:01,000 --> 00:00:02,000\nBonjour\n\n2\n00:00:03,000 --> 00:00:04,000\n12\n";
        let out = watermark_subtitles(src, "srt", "© Moi\n\nligne 2", 0.9, 0.9).unwrap();
        assert!(out.starts_with("1\n00:00:00,000 --> 99:59:59,999\n{\\an3}© Moi\nligne 2\n\n2\n"), "{out}");
        assert!(out.contains("\n3\n00:00:03,000"), "renumérotation: {out}");
        // « 12 » est du texte (pas suivi d'un timing) : intact.
        assert!(out.trim_end().ends_with("\n12"), "texte numérique modifié: {out}");
    }

    #[test]
    fn vtt_insere_apres_l_en_tete() {
        let src = "WEBVTT\nKind: captions\n\n00:01.000 --> 00:02.000\nSalut\n";
        let out = watermark_subtitles(src, "vtt", "WM", 0.5, 0.1).unwrap();
        assert!(out.starts_with("WEBVTT\nKind: captions\n\n00:00:00.000 --> 99:59:59.999 line:10% position:50% align:center\nWM\n"), "{out}");
        assert!(out.contains("00:01.000 --> 00:02.000\nSalut"), "{out}");
        assert!(watermark_subtitles("pas du vtt", "vtt", "x", 0.5, 0.5).is_err());
    }

    #[test]
    fn audio_mixe_a_intervalle() {
        // Piste mono 1 kHz, 10 s de silence ; son stéréo 2 kHz de 0,1 s à 1000.
        let mut track = DecodedAudio { samples: vec![0; 10_000], channels: 1, sample_rate: 1000 };
        let sound = DecodedAudio { samples: vec![1000; 400], channels: 2, sample_rate: 2000 };
        let n = mix(&mut track, &sound, &AudioMark { interval_s: 3.0, offset_s: 1.0, volume: 0.5 }).unwrap();
        assert_eq!(n, 3, "passages à 1, 4 et 7 s (10 s = fin de piste, exclue)");
        assert_eq!(track.samples[1000], 500, "volume appliqué");
        assert_eq!(track.samples[1099], 500, "durée conservée au rééchantillonnage");
        assert_eq!(track.samples[1100], 0, "pas de débordement");
        assert_eq!(track.samples[4000], 500);
        assert_eq!(track.samples[2000], 0);
    }

    #[test]
    fn audio_sature_sans_boucler() {
        let mut track = DecodedAudio { samples: vec![30_000; 100], channels: 1, sample_rate: 100 };
        let sound = DecodedAudio { samples: vec![30_000; 10], channels: 1, sample_rate: 100 };
        mix(&mut track, &sound, &AudioMark { interval_s: 0.0, offset_s: 0.0, volume: 1.0 }).unwrap();
        assert_eq!(track.samples[0], i16::MAX);
        assert_eq!(track.samples[10], 30_000, "intervalle 0 = un seul passage");
    }
}
