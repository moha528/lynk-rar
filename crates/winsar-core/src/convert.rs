//! Conversion d'une archive vers un autre format : RAR → 7z, ZIP → TAR.ZST…
//!
//! Le contenu est extrait dans un dossier de travail caché à côté de la
//! destination, puis réarchivé. L'original n'est jamais modifié.

use std::{fs, path::Path, path::PathBuf};

use crate::{
    CreateOptions, Error, ExtractOptions, Overwrite, Progress, Result, cancelled_or, paths, progress::TwoSteps,
};

#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Mot de passe de l'archive d'origine.
    pub password: Option<String>,
    /// Format, niveau, chiffrement et volumes de la nouvelle archive.
    pub create: CreateOptions,
}

/// Convertit `archive` en `dest` et rend les fichiers écrits (`dest` ou ses volumes).
pub fn convert(archive: &Path, dest: &Path, opts: &ConvertOptions, progress: &dyn Progress) -> Result<Vec<PathBuf>> {
    let same = |a: &Path, b: &Path| match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    // Vérifié avant l'extraction, qui peut être longue.
    let format = opts.create.format;
    if !format.can_create() {
        return Err(Error::CannotCreate(format));
    }
    if opts.create.password.is_some() && !format.can_encrypt() {
        return Err(Error::Unsupported("ce format ne gère pas le chiffrement"));
    }
    if crate::volumes(archive).iter().any(|v| same(v, dest)) {
        return Err(Error::Unsupported("la nouvelle archive ne peut pas remplacer l'originale"));
    }
    let dir = dest.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let work = paths::unique(dir, ".winsar-convert", "");
    let steps = TwoSteps::new(progress);
    let result = (|| {
        let extract =
            ExtractOptions { password: opts.password.clone(), overwrite: Overwrite::Replace, selection: None };
        crate::extract(archive, &work, &extract, &steps)?;
        steps.next();
        let sources = paths::children(&work)?;
        if sources.is_empty() {
            return Err(Error::Unsupported("l'archive est vide"));
        }
        crate::create(&sources, dest, &opts.create, &steps)
    })();
    let _ = fs::remove_dir_all(&work);
    result.map_err(|e| cancelled_or(e, progress))
}
