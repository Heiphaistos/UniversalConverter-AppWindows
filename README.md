# UniversalConverter

![Version](https://img.shields.io/badge/version-1.12.1-blue)
![Platform](https://img.shields.io/badge/platform-Windows-lightgrey)
![Stack](https://img.shields.io/badge/stack-Tauri%20v2%20%2B%20React%20%2B%20Rust-orange)
![License](https://img.shields.io/badge/license-Propriétaire-red)

**Convertisseur de fichiers universel** — Application de bureau portable pour Windows, entièrement offline, propulsée par Rust.

Convertissez images, documents, tableurs, PDF et données structurées sans jamais envoyer vos fichiers sur internet.

---

## Fonctionnalités

### Formats supportés

| Catégorie | Entrée | Sortie |
|-----------|--------|--------|
| **Images raster** | PNG, JPG, WebP, BMP, GIF, TIFF, TGA, PNM/PPM, HDR, ICO, QOI, EXR, DDS, FF | PNG, JPG, WebP, BMP, GIF, TIFF, TGA, ICO, AVIF, QOI, EXR, PPM, FF, PDF |
| **Vectoriel** | SVG | PNG, JPG, WebP, BMP, AVIF, QOI, PDF |
| **Documents** (pivot texte générique) | PDF, TXT, MD, HTML, DOCX, RTF, EPUB, ODT, PPTX, ODP | TXT, HTML, MD, PDF, DOCX, EPUB, RTF, ODT |
| **Tableurs** | XLSX, XLS, ODS | CSV, JSON, YAML, TOML, XML, XLSX, TXT, HTML, MD, PDF |
| **Données structurées** | CSV, JSON, YAML, TOML, XML | CSV, JSON, YAML, TOML, XML, XLSX, TXT, HTML, MD, PDF (combinaisons selon format source) |
| **Sous-titres** | SRT, VTT | SRT, VTT, TXT |
| **Archives** | ZIP, TAR, TGZ/GZ, 7z | ZIP, TAR, TGZ |
| **Audio** | MP3, OGG, M4A, AAC, WAV, FLAC | WAV, FLAC |

### Outils PDF intégrés
- **Fusion** — combine plusieurs PDFs en un seul (mode pages réelles ou mode condensé)
- **Découpe** — extrait des pages spécifiques (ex : `1,3,5-8`)
- **Images → PDF** — assemble plusieurs images en un PDF

### Studio filigrane
- **Tous les formats, format d'origine conservé** — une photo PNG reste un PNG, un DOCX reste un DOCX
- **Placement à la main en temps réel** — glisser pour déplacer, poignées pour la taille et la rotation, molette, flèches, aimant au centre, 9 ancrages
- **Texte ou logo** — police installée ou importée (.ttf/.otf/.woff), gras, italique, espacement, interligne, alignement
- **Couleur** — unie, dégradé linéaire (angle libre) ou radial avec autant d'arrêts que voulu, opacité par couleur, contour, ombre portée
- **Mosaïque** — répétition sur toute la surface, écarts et quinconce réglables ; miroir horizontal/vertical
- **Préréglages** — intégrés et personnels, annuler/rétablir (Ctrl+Z / Ctrl+Y)
- **Traitement par lot** — pages PDF au choix, pages de formats différents gérées (portrait, paysage, `/Rotate`)

| Famille | Ce qui est fait |
|---|---|
| Images (PNG, JPG, WebP, BMP, GIF, TIFF, TGA, ICO, QOI, AVIF, EXR, PPM, Farbfeld…) | calque incrusté en pleine résolution, même format (HDR/DDS : PNG, faute d'encodeur) |
| SVG, HTML | calque intégré au fichier |
| PDF | calque posé sur les pages choisies |
| DOCX, PPTX, XLSX, ODT, ODS, ODP, EPUB | image intégrée : en-tête Word, chaque diapositive, fond de feuille (écran), fond de page, fond des chapitres |
| TXT, MD, YAML, TOML, XML, RTF | texte du filigrane inscrit dans la syntaxe du format |
| SRT, VTT | sous-titre permanent placé selon la position choisie |
| MP3, WAV, FLAC, OGG, M4A, AAC | tatouage sonore : son choisi mixé à intervalle réglable, sortie WAV/FLAC |
| ZIP, TAR, TGZ, GZ, 7Z | chaque fichier interne traité dans son format, archive reconstruite (7Z → ZIP) |
| CSV, JSON, DOC, PPT, XLS | aucun filigrane possible sans altérer les données : PDF uniquement sur option |

### Traitement d'images
- **Qualité** — slider de compression JPEG (1–100 %)
- **Redimensionnement** — largeur et/ou hauteur en pixels (aspect ratio libre)
- **Rotation** — 0° / 90° / 180° / 270°

### Interface
- **Glisser-déposer** — déposez vos fichiers directement dans la fenêtre
- **Multi-fichiers** — convertissez plusieurs fichiers en parallèle
- **Aperçu miniature** — prévisualisation automatique avant conversion
- **Format par défaut mémorisé** — l'app retient le dernier format utilisé par extension
- **Historique** — liste des conversions récentes avec accès rapide au dossier de sortie
- **ZIP** — regroupez plusieurs fichiers convertis en une seule archive

---

## Architecture

```
UniversalConverter/
├── src/                        # Frontend React + TypeScript
│   ├── App.tsx                 # Orchestrateur principal
│   ├── types.ts                # Types partagés + helpers (formatBytes, parsePageRange)
│   └── components/
│       ├── FileUploader.tsx    # Drop zone + sélection fichier
│       ├── FileList.tsx        # Ligne de conversion par fichier
│       ├── MergePDF.tsx        # Outil fusion PDF
│       ├── MergePromptModal.tsx
│       └── History.tsx         # Historique des conversions
├── src-tauri/
│   └── src/
│       ├── commands.rs         # Commandes Tauri exposées au frontend
│       ├── conversion_engine.rs # Images raster + SVG (crate `image`, `resvg`)
│       ├── pdf_engine.rs       # PDF (lopdf, printpdf)
│       ├── office_engine.rs    # DOCX, PPTX, XLSX, CSV (quick-xml, calamine, csv)
│       └── text_engine.rs      # TXT, HTML, Markdown (pulldown-cmark)
└── build.bat                   # Script de build production
```

### Flux de conversion

```
Fichier déposé
    │
    ├── Frontend détecte l'extension
    ├── Appelle get_formats_for_extension (Rust)
    ├── Utilisateur choisit le format cible
    │
    └── invoke("convert_file") ──▶ commands.rs
            │
            ├── Vérification taille (max 500 MB)
            ├── Validation chemin sortie (anti path traversal)
            ├── Routage selon (ext_entrée, fmt_sortie)
            │       ├── conversion_engine.rs  (images)
            │       ├── pdf_engine.rs         (PDF)
            │       ├── office_engine.rs      (Office)
            │       └── text_engine.rs        (texte)
            └── Retourne { path, output_size }
```

---

## Installation

### Option 1 : Portable (recommandée)

1. Télécharger `Universal.converter.7z` depuis les [Releases](https://github.com/Heiphaistos/UniversalConverter-AppWindows/releases/latest)
2. Extraire l'archive
3. Lancer `UniversalConverter.exe`

### Option 2 : Depuis les sources

**Prérequis** : Node.js 18+, Rust 1.75+, npm

```bash
git clone https://github.com/Heiphaistos/UniversalConverter-AppWindows.git
cd UniversalConverter-AppWindows
npm install
npm run tauri dev
```

### Option 3 : Build production

```bash
build.bat
```

L'exécutable est généré dans `src-tauri/target/release/bundle/`.

---

## Sécurité

- **100 % offline** — aucune donnée envoyée sur internet
- **Protection path traversal** — séquences `..` bloquées dans tous les chemins de sortie
- **Limite de taille** — fichiers > 500 MB rejetés
- **Fichiers temporaires** — supprimés automatiquement via RAII (`TempFile` guard Rust)
- **CSP stricte** — Content Security Policy configurée dans tauri.conf.json

---

## Technologies

| Couche | Technologie |
|--------|------------|
| Runtime | Tauri v2 |
| Frontend | React 18 + TypeScript + Vite |
| Backend | Rust 2021 |
| Images | crate `image` v0.25 (PNG/JPG/WebP/BMP/GIF/TIFF/AVIF/HDR/TGA) |
| SVG | `resvg` v0.44 |
| PDF | `lopdf` v0.33 + `printpdf` v0.7 |
| Office | `quick-xml` v0.36 + `calamine` v0.26 + `rust_xlsxwriter` v0.93 |
| Markdown | `pulldown-cmark` v0.11 |
| Data (CSV/JSON/YAML/TOML/XML) | `csv` v1 + `serde_json` + `serde_yaml` + `toml` + `quick-xml` |
| Sous-titres | SRT/VTT (parseur maison) |
| Archives | `zip` v2 + `tar` + `flate2` + `sevenz-rust` |
| Audio | `symphonia` (MP3/OGG/M4A/AAC/WAV/FLAC) |
| EPUB/RTF/ODT | pipeline pivot texte générique (`doc_engine`) |

---

## Auteur

**Heiphaistos**

---

*UniversalConverter — Conversion de fichiers locale, rapide et sans dépendances externes*
