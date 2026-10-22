//! Extraction RAR via la bibliothèque UnRAR officielle (licence UnRAR : extraction seulement).

use std::{fs, path::Path};

use unrar::{
    Archive,
    error::{Code, UnrarError},
};

use crate::{Entry, Error, ExtractOptions, ExtractReport, Progress, Result, paths, progress::check, time};

fn open<'a>(path: &'a Path, pw: Option<&'a str>) -> Archive<'a> {
    match pw {
        Some(pw) => Archive::with_password(path, pw),
        None => Archive::new(path),
    }
}

fn map_err(e: UnrarError, encrypted: bool) -> Error {
    match e.code {
        Code::MissingPassword | Code::BadPassword => Error::Password,
        // RAR4 : un mauvais mot de passe se manifeste en erreur de CRC.
        Code::BadData if encrypted => Error::Password,
        _ => Error::Archive(e.to_string()),
    }
}

pub(super) fn list(path: &Path, pw: Option<&str>) -> Result<Vec<Entry>> {
    let archive = open(path, pw).open_for_listing().map_err(|e| map_err(e, false))?;
    let mut out = Vec::new();
    for header in archive {
        let h = header.map_err(|e| map_err(e, false))?;
        // Fichier à cheval sur plusieurs volumes : déjà listé dans le volume précédent.
        if h.is_split_before() {
            continue;
        }
        let mut e = Entry::new(&h.filename.to_string_lossy(), h.is_directory(), h.unpacked_size);
        e.modified = time::dos_to_unix(h.file_time);
        e.encrypted = h.is_encrypted();
        out.push(e);
    }
    Ok(out)
}

pub(super) fn extract(
    path: &Path,
    dest: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
) -> Result<ExtractReport> {
    let pw = opts.password.as_deref();
    progress.total(list(path, pw)?.iter().filter(|e| opts.selects(&e.path)).map(|e| e.size).sum());

    let mut report = ExtractReport::default();
    let mut archive = open(path, pw).open_for_processing().map_err(|e| map_err(e, false))?;
    while let Some(header) = archive.read_header().map_err(|e| map_err(e, false))? {
        check(progress)?;
        let entry = header.entry();
        let name = entry.filename.to_string_lossy().replace('\\', "/");
        let (size, is_dir, encrypted) = (entry.unpacked_size, entry.is_directory(), entry.is_encrypted());
        let Some(effective) = opts.effective_name(&name) else {
            archive = header.skip().map_err(|e| map_err(e, encrypted))?;
            continue;
        };
        progress.entry(&name);

        let next = match paths::safe_relative(&effective).map(|rel| dest.join(rel)) {
            None => {
                report.skipped.push(name);
                header.skip()
            }
            Some(target) if is_dir => {
                fs::create_dir_all(&target)?;
                report.dirs += 1;
                header.skip()
            }
            Some(target) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                match paths::resolve_target(target, opts.overwrite) {
                    None => {
                        report.skipped.push(name);
                        header.skip()
                    }
                    Some(target) => {
                        let next = header.extract_to(&target);
                        if next.is_ok() {
                            report.files += 1;
                            report.bytes += size;
                        }
                        next
                    }
                }
            }
        };
        progress.advance(size);
        archive = next.map_err(|e| map_err(e, encrypted))?;
    }
    Ok(report)
}

pub(super) fn test(path: &Path, pw: Option<&str>, progress: &dyn Progress) -> Result<crate::TestReport> {
    progress.total(list(path, pw)?.iter().map(|e| e.size).sum());
    let mut report = crate::TestReport::default();
    let mut archive = open(path, pw).open_for_processing().map_err(|e| map_err(e, false))?;
    while let Some(header) = archive.read_header().map_err(|e| map_err(e, false))? {
        check(progress)?;
        let entry = header.entry();
        let name = entry.filename.to_string_lossy().replace('\\', "/");
        let (size, is_dir, encrypted) = (entry.unpacked_size, entry.is_directory(), entry.is_encrypted());
        progress.entry(&name);
        if is_dir {
            archive = header.skip().map_err(|e| map_err(e, encrypted))?;
            continue;
        }
        match header.test() {
            Ok(next) => {
                report.files += 1;
                report.bytes += size;
                progress.advance(size);
                archive = next;
            }
            Err(e) => match map_err(e, encrypted) {
                Error::Password => return Err(Error::Password),
                // UnRAR ne permet pas de poursuivre après une erreur de données.
                e => {
                    report.errors.push(format!("{name} : {e}"));
                    break;
                }
            },
        }
    }
    Ok(report)
}
