use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use walkdir::WalkDir;

use crate::{Overwrite, Result};

/// Un élément à archiver, avec son nom dans l'archive (séparé par `/`).
pub(crate) struct Item {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Développe les sources en liste plate. Un dossier sélectionné garde son nom
/// comme racine dans l'archive, comme dans l'Explorateur. Les liens symboliques
/// sont ignorés pour ne jamais archiver hors de la sélection.
pub(crate) fn collect(sources: &[PathBuf]) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for src in sources {
        let root_name =
            src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "archive".to_owned());
        for entry in WalkDir::new(src).follow_links(false).sort_by_file_name() {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                continue;
            }
            let rel = entry.path().strip_prefix(src).unwrap_or(Path::new(""));
            let mut name = root_name.clone();
            for part in rel.components() {
                name.push('/');
                name.push_str(&part.as_os_str().to_string_lossy());
            }
            let meta = entry.metadata()?;
            items.push(Item {
                path: entry.path().to_path_buf(),
                name,
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified: meta.modified().ok(),
            });
        }
    }
    Ok(items)
}

/// Chemin relatif sûr pour une entrée d'archive : pas de `..`, pas de chemin absolu
/// ni de lecteur, caractères interdits par Windows remplacés. `None` si rien ne reste.
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for (i, part) in name.split(['/', '\\']).enumerate() {
        match part {
            "" | "." => continue,
            ".." => return None,
            // `C:` en tête : chemin absolu Windows, on retire le lecteur.
            p if i == 0 && p.len() == 2 && p.ends_with(':') => continue,
            p => out.push(sanitize(p)),
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// `name` est `item` lui-même ou se trouve dedans (`a/b` contient `a/b/c`, pas `a/bc`).
pub(crate) fn is_within(name: &str, item: &str) -> bool {
    let item = item.trim_matches('/');
    name == item || name.strip_prefix(item).is_some_and(|rest| rest.starts_with('/'))
}

fn sanitize(part: &str) -> String {
    let mut s: String = part
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = s.trim_end_matches(['.', ' ']).len();
    s.truncate(trimmed);
    if s.is_empty() {
        return "_".to_owned();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        s.insert(0, '_');
    }
    s
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

/// `parent/base{ext}`, puis `parent/base (2){ext}`, etc. jusqu'à un nom libre.
pub(crate) fn unique(parent: &Path, base: &str, ext: &str) -> PathBuf {
    (1u32..)
        .map(|n| match n {
            1 => parent.join(format!("{base}{ext}")),
            n => parent.join(format!("{base} ({n}){ext}")),
        })
        .find(|p| !exists(p))
        .expect("espace de noms épuisé")
}

/// Cible finale d'un fichier extrait selon la politique d'écrasement ; `None` = ignorer.
pub(crate) fn resolve_target(path: PathBuf, overwrite: Overwrite) -> Option<PathBuf> {
    if !exists(&path) {
        return Some(path);
    }
    match overwrite {
        Overwrite::Skip => None,
        Overwrite::Replace => Some(path),
        Overwrite::Rename => {
            let parent = path.parent().unwrap_or(Path::new(""));
            let name = path.file_name()?.to_string_lossy();
            let (base, ext) = match name.rfind('.') {
                Some(i) if i > 0 => (&name[..i], &name[i..]),
                _ => (&name[..], ""),
            };
            Some(unique(parent, base, ext))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert_eq!(safe_relative("../etc/passwd"), None);
        assert_eq!(safe_relative("a/../../b"), None);
        assert_eq!(safe_relative("/"), None);
    }

    #[test]
    fn strips_absolute() {
        assert_eq!(safe_relative("/abs/x.txt"), Some(PathBuf::from("abs/x.txt")));
        assert_eq!(safe_relative("C:\\Windows\\x"), Some(PathBuf::from("Windows/x")));
    }

    #[test]
    fn sanitizes_windows_names() {
        assert_eq!(safe_relative("a:b?.txt"), Some(PathBuf::from("a_b_.txt")));
        assert_eq!(safe_relative("CON.txt"), Some(PathBuf::from("_CON.txt")));
        assert_eq!(safe_relative("dot. "), Some(PathBuf::from("dot")));
    }
}
