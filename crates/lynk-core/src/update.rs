//! Modification d'une archive existante : ajout et suppression d'éléments.
//!
//! On écrit toujours une nouvelle archive à côté, puis on la substitue à
//! l'ancienne : une erreur ou une annulation en cours de route laisse
//! l'original intact.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use crate::{
    CreateOptions, Error, ExtractOptions, Format, Overwrite, Progress, Result, backends, cancelled_or,
    paths::{self, safe_relative},
    progress::TwoSteps,
};

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// Requis pour lire une archive chiffrée ; les ajouts sont chiffrés avec.
    pub password: Option<String>,
    /// Niveau des fichiers ajoutés (et de tout le contenu hors ZIP, recompressé).
    pub level: u32,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self { password: None, level: 6 }
    }
}

/// Ajoute `add` dans le dossier `base` de l'archive (`""` pour la racine) et retire
/// les entrées `remove` (fichiers ou dossiers entiers). Un élément ajouté remplace
/// l'entrée de même chemin ; un dossier ajouté fusionne avec l'existant.
pub fn update(
    archive: &Path,
    base: &str,
    add: &[PathBuf],
    remove: &[String],
    opts: &UpdateOptions,
    progress: &dyn Progress,
) -> Result<()> {
    let format = Format::detect(archive)?;
    if !format.can_create() {
        return Err(Error::CannotCreate(format));
    }
    if crate::volumes(archive).len() > 1 {
        return Err(Error::Unsupported("une archive en volumes ne peut pas être modifiée"));
    }
    if opts.password.is_some() && !format.can_encrypt() {
        return Err(Error::Unsupported("ce format ne gère pas le chiffrement"));
    }
    let base = base.trim_matches('/');
    let parent = archive.parent().unwrap_or(Path::new("."));
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = paths::unique(parent, &format!(".{name}.lynk"), ".tmp");

    let result = match format {
        Format::Zip => {
            let mut items = paths::collect(add)?;
            if !base.is_empty() {
                for it in &mut items {
                    it.name = format!("{base}/{}", it.name);
                }
            }
            let added: HashSet<&str> = items.iter().map(|it| it.name.as_str()).collect();
            let doomed = |entry: &str| {
                let entry = entry.replace('\\', "/");
                let entry = entry.trim_matches('/');
                added.contains(entry) || remove.iter().any(|r| paths::is_within(entry, r))
            };
            backends::zip_update(archive, &tmp, &items, &doomed, opts, progress)
        }
        _ => rebuild(format, archive, &tmp, base, add, remove, opts, progress),
    };

    match result {
        Ok(()) => {
            // MoveFileEx avec remplacement : la bascule est atomique sur un même volume.
            fs::rename(&tmp, archive)?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(cancelled_or(e, progress))
        }
    }
}

/// Formats sans copie brute possible (7z, tar.*) : extraction, modification, recréation.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    format: Format,
    archive: &Path,
    tmp: &Path,
    base: &str,
    add: &[PathBuf],
    remove: &[String],
    opts: &UpdateOptions,
    progress: &dyn Progress,
) -> Result<()> {
    let parent = archive.parent().unwrap_or(Path::new("."));
    let work = paths::unique(parent, ".lynk-work", "");
    let steps = TwoSteps::new(progress);
    let result = (|| {
        let extract =
            ExtractOptions { password: opts.password.clone(), overwrite: Overwrite::Replace, selection: None };
        crate::extract(archive, &work, &extract, &steps)?;
        steps.next();
        for r in remove {
            if let Some(rel) = safe_relative(r) {
                let p = work.join(rel);
                if p.is_dir() {
                    fs::remove_dir_all(&p)?;
                } else if p.exists() {
                    fs::remove_file(&p)?;
                }
            }
        }
        let target = safe_relative(base).map_or_else(|| work.clone(), |rel| work.join(rel));
        fs::create_dir_all(&target)?;
        for src in add {
            copy_into(src, &target)?;
        }
        let create = CreateOptions { format, level: opts.level, password: opts.password.clone(), volume_size: None };
        crate::create(&paths::children(&work)?, tmp, &create, &steps).map(drop)
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

/// Copie `src` (fichier ou dossier) dans `dir`, en fusionnant avec l'existant.
fn copy_into(src: &Path, dir: &Path) -> Result<()> {
    let name = src.file_name().ok_or(Error::Unsupported("source sans nom"))?;
    if !src.is_dir() {
        fs::copy(src, dir.join(name))?;
        return Ok(());
    }
    for entry in WalkDir::new(src).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            continue;
        }
        let rel = entry.path().strip_prefix(src).unwrap_or(Path::new(""));
        let dest = dir.join(name).join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
        } else {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}
