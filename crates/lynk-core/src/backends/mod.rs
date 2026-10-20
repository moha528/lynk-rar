mod rar;
mod sevenz;
mod tar;
mod zip;

use std::{
    fs::File,
    io::{self, BufWriter, Read},
    path::Path,
    time::SystemTime,
};

use crate::{CreateOptions, Entry, ExtractOptions, ExtractReport, Format, Progress, Result, paths::Item};

pub(crate) fn list(format: Format, path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    match format {
        Format::Zip => zip::list(path),
        Format::SevenZ => sevenz::list(path, password),
        Format::Rar => rar::list(path, password),
        _ => tar::list(format, path),
    }
}

pub(crate) fn extract(
    format: Format,
    path: &Path,
    dest: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
) -> Result<ExtractReport> {
    match format {
        Format::Zip => zip::extract(path, dest, opts, progress),
        Format::SevenZ => sevenz::extract(path, dest, opts, progress),
        Format::Rar => rar::extract(path, dest, opts, progress),
        _ => tar::extract(format, path, dest, opts, progress),
    }
}

pub(crate) fn create(
    format: Format,
    items: &[Item],
    dest: &Path,
    opts: &CreateOptions,
    progress: &dyn Progress,
) -> Result<()> {
    match format {
        Format::Zip => zip::create(items, dest, opts, progress),
        Format::SevenZ => sevenz::create(items, dest, opts, progress),
        Format::Rar => unreachable!("filtré par Format::can_create"),
        _ => tar::create(format, items, dest, opts, progress),
    }
}

pub(crate) fn test(
    format: Format,
    path: &Path,
    password: Option<&str>,
    progress: &dyn Progress,
) -> Result<crate::TestReport> {
    match format {
        Format::Zip => zip::test(path, password, progress),
        Format::SevenZ => sevenz::test(path, password, progress),
        Format::Rar => rar::test(path, password, progress),
        _ => tar::test(format, path, progress),
    }
}

/// Réécriture d'un ZIP par copie brute des entrées conservées (sans recompression).
pub(crate) fn zip_update(
    archive: &Path,
    tmp: &Path,
    items: &[Item],
    doomed: &dyn Fn(&str) -> bool,
    opts: &crate::UpdateOptions,
    progress: &dyn Progress,
) -> Result<()> {
    zip::update(archive, tmp, items, doomed, opts, progress)
}

/// Écrit un fichier extrait puis lui rend sa date de modification.
pub(crate) fn write_file(target: &Path, reader: &mut dyn Read, modified: Option<SystemTime>) -> Result<u64> {
    let mut out = BufWriter::with_capacity(1 << 20, File::create(target)?);
    let n = io::copy(reader, &mut out)?;
    let file = out.into_inner().map_err(io::IntoInnerError::into_error)?;
    if let Some(t) = modified {
        let _ = file.set_modified(t);
    }
    Ok(n)
}
