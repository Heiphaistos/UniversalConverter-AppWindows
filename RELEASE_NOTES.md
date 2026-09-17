# Release Notes — UniversalConverter

---

## v1.12.1 — 2026-09-17

### Corrections
- Tatouage sonore : un premier passage réglé au-delà de la durée de la piste faisait échouer le fichier ; la marque est maintenant posée au début de la piste

---

## v1.12.0 — 2026-09-17

### Nouveautés
- **Studio filigrane** — filigrane personnalisable sur tous les formats pris en charge, en conservant le format d'origine
- **Aperçu temps réel et placement à la souris** — déplacement, taille, rotation, aimant, ancrages, flèches du clavier
- **Personnalisation complète** — texte ou logo, polices installées ou importées, couleur unie, dégradé linéaire ou radial, contour, ombre, opacité, mosaïque, miroir
- **Préréglages et historique** — préréglages intégrés/personnels, annuler/rétablir
- **Documents** — DOCX, PPTX, XLSX, ODT, ODS, ODP, EPUB, HTML, SVG reçoivent le filigrane dans leur propre format
- **Texte, sous-titres, audio, archives** — texte inscrit, sous-titre permanent, tatouage sonore, contenu d'archive traité fichier par fichier

### Corrections
- Le résultat de conversion exposait `output_size` au lieu de `outputSize` : la taille des fichiers convertis était absente de l'historique

---

## v1.7.0 — 2026-05-19

### Nouveautés
- **README complet** — Documentation complète : formats, architecture, flux de conversion, sécurité
- **RELEASE_NOTES** — Historique des versions

### Synchronisation
- Version alignée à 1.7.0 dans tous les fichiers (package.json, Cargo.toml, tauri.conf.json, build.bat)

---

## v1.6.0 — 2026-04-01

### Sécurité
- Protection **path traversal** : séquences `..` bloquées dans tous les chemins de sortie (`validate_output_path`)
- Limite taille fichier : **500 MB** (rejet en amont avant traitement)
- Fichiers temporaires supprimés via **RAII** (`TempFile` guard Rust)
- CSP stricte configurée dans `tauri.conf.json`

### Corrections
- Correction de la **mémoire de format par défaut** : le dernier format utilisé par extension est maintenant persisté côté localStorage
- Fix zip : **déduplication des noms** d'entrée (`file.txt` → `file_2.txt`) pour éviter les collisions dans l'archive
- Fix : `convert_file` retourne une erreur claire si le fichier de sortie n'a pas été créé

### Qualité
- Refactoring moteur : séparation en 4 modules (`conversion_engine`, `pdf_engine`, `office_engine`, `text_engine`)
- Suppression des `unwrap()` non contrôlés dans les moteurs de conversion

---

## v1.5.0 — 2026-03-15

### Nouveautés
- Conversion **AVIF** (encodage/décodage via crate `image`)
- Support **HDR** et **PNM**
- Export **XLSX** depuis CSV (`rust_xlsxwriter`)
- Conversion **JSON → CSV** (tableau d'objets → CSV avec en-têtes)

---

## v1.4.0 — 2026-01-20

### Nouveautés
- **Fusion PDF** — mode pages réelles (`merge_pdfs_pages`) et mode condensé (`merge_pdfs_single_page`)
- **Découpe PDF** — sélection de pages par plage (`1,3,5-8`)
- **Images → PDF** — assembly de plusieurs images en un PDF
- **ZIP** — regroupement de fichiers en archive (avec déduplication des noms)
- Support **PPTX/PPT → TXT/PDF**
- Support **XLSX/XLS/ODS → PDF** (via intermédiaire TXT)
- Support **CSV → PDF** (via intermédiaire TXT)

### Corrections
- Fix parsing plages PDF : erreur explicite si début > fin, si page hors bornes
- Fix `excel_to_csv` : séparateur décimal cohérent pour les nombres flottants

---

## v1.0.0 — 2025-12-01

### Version initiale
- Conversion images raster : PNG, JPG, WebP, BMP, GIF, TIFF, TGA, ICO
- Conversion SVG → raster (PNG, JPG, WebP, BMP)
- Conversion PDF → TXT
- Conversion TXT/HTML/Markdown → PDF
- Conversion DOCX → TXT/HTML/PDF
- Conversion XLSX/XLS/ODS → CSV/JSON/TXT
- Conversion CSV → JSON/XLSX/TXT
- Drag & drop + multi-fichiers
- Historique des conversions
- Aperçu miniature automatique
