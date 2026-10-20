//! Moteur d'archives de Lynk : détection, listing, extraction et création.
//!
//! Indépendant de toute interface : le CLI, l'application et l'extension de
//! l'Explorateur passent tous par ces fonctions.

mod backends;
mod entry;
mod error;
mod format;
mod paths;
mod progress;
mod source;
mod time;
mod update;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub use entry::Entry;
pub use error::{Error, Result};
pub use format::Format;
pub use paths::safe_relative;
pub use progress::{NoProgress, Progress};
pub use source::volumes;
pub use update::{UpdateOptions, update};

/// Que faire quand un fichier extrait existe déjà.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Overwrite {
    Skip,
    Replace,
    /// Garde les deux : `rapport (2).pdf`. Le plus sûr, donc par défaut.
    #[default]
    Rename,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    pub password: Option<String>,
    pub overwrite: Overwrite,
    /// `None` : toute l'archive.
    pub selection: Option<Selection>,
}

/// Extraction partielle, telle que la fait un explorateur d'archive : les éléments
/// choisis (fichiers ou dossiers, chemins complets) sont extraits relativement au
/// dossier affiché `base`. Extraire `docs/img` depuis `docs` donne `img/…`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Selection {
    pub base: String,
    pub items: Vec<String>,
}

impl ExtractOptions {
    /// Nom de l'entrée une fois la sélection appliquée, `None` si elle n'est pas retenue.
    pub(crate) fn effective_name(&self, raw: &str) -> Option<String> {
        let name = raw.replace('\\', "/");
        let name = name.trim_matches('/');
        let Some(sel) = &self.selection else {
            return Some(name.to_owned());
        };
        if !sel.items.iter().any(|i| paths::is_within(name, i)) {
            return None;
        }
        match sel.base.trim_matches('/') {
            "" => Some(name.to_owned()),
            base => name.strip_prefix(base).and_then(|rest| rest.strip_prefix('/')).map(str::to_owned),
        }
    }

    pub(crate) fn selects(&self, raw: &str) -> bool {
        self.selection.is_none() || self.effective_name(raw).is_some()
    }
}

#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub format: Format,
    /// 0 (stockage) à 9 (maximum).
    pub level: u32,
    pub password: Option<String>,
    /// Découpe en volumes `nom.ext.001`, `.002`… de cette taille (format de 7-Zip).
    pub volume_size: Option<u64>,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self { format: Format::Zip, level: 6, password: None, volume_size: None }
    }
}

/// Résultat d'un test d'intégrité : l'archive est saine si `errors` est vide.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub files: u64,
    pub bytes: u64,
    pub errors: Vec<String>,
}

impl TestReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractReport {
    pub destination: PathBuf,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// Entrées ignorées : chemins dangereux, liens, fichiers déjà présents en mode `Skip`.
    pub skipped: Vec<String>,
}

pub fn list(archive: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let format = Format::detect(archive)?;
    backends::list(format, archive, password)
}

pub fn extract(archive: &Path, dest: &Path, opts: &ExtractOptions, progress: &dyn Progress) -> Result<ExtractReport> {
    let format = Format::detect(archive)?;
    std::fs::create_dir_all(dest)?;
    let mut report = backends::extract(format, archive, dest, opts, progress).map_err(|e| cancelled_or(e, progress))?;
    report.destination = dest.to_path_buf();
    Ok(report)
}

/// « Extraire ici » intelligent : directement à côté de l'archive si elle ne contient
/// qu'un seul élément racine, sinon dans un dossier portant le nom de l'archive.
pub fn extract_smart(archive: &Path, opts: &ExtractOptions, progress: &dyn Progress) -> Result<ExtractReport> {
    extract_smart_in(archive, archive.parent().unwrap_or(Path::new(".")), opts, progress)
}

/// Extraction intelligente dans un dossier choisi.
pub fn extract_smart_in(
    archive: &Path,
    dir: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
) -> Result<ExtractReport> {
    let entries: Vec<Entry> = list(archive, opts.password.as_deref())?
        .into_iter()
        .filter_map(|mut e| {
            e.path = opts.effective_name(&e.path)?;
            Some(e)
        })
        .collect();
    let dest = smart_destination(archive, dir, &entries);
    extract(archive, &dest, opts, progress)
}

/// Crée l'archive et rend les fichiers écrits : `dest` seul, ou ses volumes.
pub fn create(sources: &[PathBuf], dest: &Path, opts: &CreateOptions, progress: &dyn Progress) -> Result<Vec<PathBuf>> {
    if !opts.format.can_create() {
        return Err(Error::CannotCreate(opts.format));
    }
    if opts.password.is_some() && !opts.format.can_encrypt() {
        return Err(Error::Unsupported("ce format ne gère pas le chiffrement"));
    }
    let items = paths::collect(sources)?;
    if let Err(e) = backends::create(opts.format, &items, dest, opts, progress) {
        // Jamais d'archive à moitié écrite.
        let _ = std::fs::remove_file(dest);
        return Err(cancelled_or(e, progress));
    }
    match opts.volume_size {
        Some(size) => source::split_into_volumes(dest, size),
        None => Ok(vec![dest.to_path_buf()]),
    }
}

/// Vérifie l'archive sans rien écrire : tout est décompressé et les sommes de
/// contrôle (CRC, intégrité des flux) sont vérifiées. Les erreurs par fichier
/// sont collectées ; une erreur d'ouverture ou de mot de passe est renvoyée.
pub fn test(archive: &Path, password: Option<&str>, progress: &dyn Progress) -> Result<TestReport> {
    let format = Format::detect(archive)?;
    backends::test(format, archive, password, progress).map_err(|e| cancelled_or(e, progress))
}

/// Les erreurs d'E/S peuvent masquer l'annulation (une bibliothèque qui réemballe l'erreur).
pub(crate) fn cancelled_or(e: Error, progress: &dyn Progress) -> Error {
    if progress.is_cancelled() { Error::Cancelled } else { e }
}

/// Dossier libre au nom de l'archive dans `dir` : `photos.tar.gz` → `dir/photos`.
pub fn named_destination(archive: &Path, dir: &Path) -> PathBuf {
    let name = source::logical_name(archive).unwrap_or_default();
    let stem = match Format::strip_extension(&name) {
        "" => "archive",
        s => s,
    };
    paths::unique(dir, stem, "")
}

/// `dir` lui-même si les entrées n'ont qu'une racine encore absente de `dir`,
/// sinon un dossier au nom de l'archive : on n'éparpille jamais des fichiers.
pub fn smart_destination(archive: &Path, dir: &Path, entries: &[Entry]) -> PathBuf {
    let roots: HashSet<&str> =
        entries.iter().filter_map(|e| e.path.split('/').find(|p| !p.is_empty() && *p != ".")).collect();
    if roots.len() == 1 {
        let root = roots.into_iter().next().unwrap_or_default();
        if let Some(rel) = safe_relative(root)
            && !dir.join(rel).exists()
        {
            return dir.to_path_buf();
        }
    }
    named_destination(archive, dir)
}

/// Nom proposé pour « Ajouter à l'archive » : `rapport.zip` pour un seul élément,
/// le nom du dossier parent pour une sélection multiple. Jamais un fichier existant.
pub fn default_archive_path(sources: &[PathBuf], format: Format) -> PathBuf {
    let Some(first) = sources.first() else {
        return PathBuf::from(format!("archive.{}", format.extension()));
    };
    let parent = first.parent().unwrap_or(Path::new("."));
    let base = if sources.len() == 1 {
        let name = first.file_name().map(|n| n.to_string_lossy().into_owned());
        if first.is_dir() {
            name
        } else if let Some(n) = name.filter(|n| Format::from_name(n).is_some()) {
            // Réarchiver `photos.tar.gz` donne `photos.zip`, pas `photos.tar.zip`.
            Some(Format::strip_extension(&n).to_owned())
        } else {
            first.file_stem().map(|n| n.to_string_lossy().into_owned())
        }
    } else {
        parent.file_name().map(|n| n.to_string_lossy().into_owned())
    }
    .filter(|n| !n.is_empty())
    .unwrap_or_else(|| "archive".to_owned());
    paths::unique(parent, &base, &format!(".{}", format.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_names() {
        let opts = ExtractOptions {
            selection: Some(Selection { base: "a".into(), items: vec!["a/b".into()] }),
            ..Default::default()
        };
        assert_eq!(opts.effective_name("a/b"), Some("b".into()));
        assert_eq!(opts.effective_name("a/b/c.txt"), Some("b/c.txt".into()));
        assert_eq!(opts.effective_name("a/bc"), None);
        assert_eq!(opts.effective_name("a"), None);
        assert_eq!(ExtractOptions::default().effective_name("\\x\\y/"), Some("x/y".into()));
    }
}
