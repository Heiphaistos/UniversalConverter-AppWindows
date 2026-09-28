//! path_guard.rs — Garde des chemins d'écriture : refuse les zones système.
//!
//! Compare des chemins CANONICALISÉS composant par composant (`Path::starts_with`),
//! jamais des chaînes : sous Windows `canonicalize` rend `\\?\C:\...`, qu'aucun
//! littéral `c:\windows` ne peut matcher.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

/// Racines système interdites en écriture, canonicalisées (celles qui n'existent pas sont ignorées).
fn forbidden_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    let raw: Vec<PathBuf> = [
        ("SystemRoot", r"C:\Windows"),
        ("ProgramFiles", r"C:\Program Files"),
        ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        ("ProgramData", r"C:\ProgramData"),
    ]
    .iter()
    .map(|(var, default)| std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default)))
    .collect();
    #[cfg(not(windows))]
    let raw: Vec<PathBuf> = ["/etc", "/usr", "/boot", "/bin", "/sbin", "/lib", "/var", "/sys", "/proc"]
        .iter()
        .map(PathBuf::from)
        .collect();
    raw.into_iter().filter_map(|p| p.canonicalize().ok()).collect()
}

/// Résout `path` en chemin canonique. S'il n'existe pas encore, canonicalise le parent
/// existant le plus proche et y rattache le reste. Échec fermé : toute erreur autre que
/// « introuvable » est refusée.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let raw = path.to_string_lossy();
    if raw.starts_with("\\\\") || raw.starts_with("//") {
        return Err("Chemin UNC refusé".to_string());
    }
    if !path.is_absolute() {
        return Err("Chemin refusé : chemin absolu requis".to_string());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Chemin refusé : séquence '..' interdite".to_string());
    }
    let mut existing = path;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(mut out) => {
                out.extend(tail.iter().rev());
                return Ok(out);
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tail.push(existing.file_name().ok_or("Chemin refusé : aucun parent existant")?);
                existing = existing.parent().ok_or("Chemin refusé : aucun parent existant")?;
            }
            Err(e) => return Err(format!("Chemin inaccessible : {e}")),
        }
    }
}

/// Valide une cible d'écriture (fichier ou dossier, existant ou non) et renvoie son chemin
/// canonique, à utiliser pour l'écriture.
pub fn check_write_target(path: &Path) -> Result<PathBuf, String> {
    let resolved = resolve(path)?;
    if let Some(root) = forbidden_roots().iter().find(|r| resolved.starts_with(r)) {
        return Err(format!(
            "Chemin refusé : écriture interdite dans une zone système protégée ({})",
            root.display()
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_dir() -> PathBuf {
        #[cfg(windows)]
        return PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("uc_test.png");
        #[cfg(not(windows))]
        return PathBuf::from("/etc/uc_test.png");
    }

    #[test]
    fn system_path_refused() {
        assert!(check_write_target(&system_dir()).is_err());
    }

    #[test]
    fn missing_subdir_under_system_refused() {
        assert!(check_write_target(&system_dir().with_file_name("nope").join("x").join("y.png")).is_err());
    }

    #[test]
    fn home_path_accepted() {
        let home = std::env::temp_dir().join("uc_guard_new_dir").join("out.png");
        let ok = check_write_target(&home).expect("chemin utilisateur refusé à tort");
        assert!(ok.ends_with("uc_guard_new_dir/out.png") || ok.ends_with(r"uc_guard_new_dir\out.png"));
    }

    #[test]
    fn parent_dir_refused() {
        let p = std::env::temp_dir().join("..").join("x.png");
        assert!(check_write_target(&p).is_err());
    }

    #[test]
    fn relative_refused() {
        assert!(check_write_target(Path::new("x.png")).is_err());
    }
}
