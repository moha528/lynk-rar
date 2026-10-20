use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sevenz_rust2::{
    ArchiveEntry, ArchiveReader, ArchiveWriter, EncoderConfiguration, EncoderMethod, Password, SourceReader,
    encoder_options::{AesEncoderOptions, Lzma2Options},
};

use crate::{
    CreateOptions, Entry, Error, ExtractOptions, ExtractReport, Progress, Result,
    paths::{self, Item},
    progress::{CancelledMarker, Tracked, check},
    source::Source,
};

fn password(pw: Option<&str>) -> Password {
    pw.map_or_else(Password::empty, Password::new)
}

fn map_err(e: sevenz_rust2::Error) -> Error {
    use sevenz_rust2::Error as E;
    match e {
        E::PasswordRequired | E::MaybeBadPassword(_) => Error::Password,
        E::Io(e, _) | E::FileOpen(e, _) => e.into(),
        e => Error::Archive(e.to_string()),
    }
}

fn modified(entry: &ArchiveEntry) -> Option<SystemTime> {
    entry.has_last_modified_date.then(|| SystemTime::from(entry.last_modified_date()))
}

fn open(path: &Path, pw: Option<&str>) -> Result<ArchiveReader<Source>> {
    ArchiveReader::new(Source::open(path)?, password(pw)).map_err(map_err)
}

pub(super) fn list(path: &Path, pw: Option<&str>) -> Result<Vec<Entry>> {
    let reader = open(path, pw)?;
    Ok(reader
        .archive()
        .files
        .iter()
        .filter(|f| !f.is_anti_item())
        .map(|f| {
            let mut e = Entry::new(f.name(), f.is_directory(), f.size());
            e.modified = modified(f).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            e
        })
        .collect())
}

pub(super) fn extract(
    path: &Path,
    dest: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
) -> Result<ExtractReport> {
    let mut reader = open(path, opts.password.as_deref())?;
    progress.total(reader.archive().files.iter().filter(|f| opts.selects(f.name())).map(|f| f.size()).sum());
    let mut report = ExtractReport::default();
    // Le rappel ne peut renvoyer que l'erreur de sevenz : on garde la nôtre à côté.
    let mut failure = None;
    let result =
        reader.for_each_entries(|entry, data| match extract_one(entry, data, dest, opts, progress, &mut report) {
            Ok(()) => Ok(true),
            Err(e) => {
                failure = Some(e);
                Ok(false)
            }
        });
    if let Some(e) = failure {
        return Err(e);
    }
    result.map_err(map_err)?;
    Ok(report)
}

fn extract_one(
    entry: &ArchiveEntry,
    data: &mut dyn Read,
    dest: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
    report: &mut ExtractReport,
) -> Result<()> {
    check(progress)?;
    let name = entry.name();
    let Some(effective) = opts.effective_name(name) else {
        // Hors sélection : dans un bloc solide, le flux doit quand même être consommé.
        io::copy(data, &mut io::sink())?;
        return Ok(());
    };
    progress.entry(name);
    if entry.is_anti_item() {
        return Ok(());
    }
    let Some(rel) = paths::safe_relative(&effective) else {
        report.skipped.push(name.to_owned());
        return Ok(());
    };
    let target = dest.join(rel);
    if entry.is_directory() {
        fs::create_dir_all(&target)?;
        report.dirs += 1;
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut data = Tracked::new(data, progress);
    let Some(target) = paths::resolve_target(target, opts.overwrite) else {
        // Archive solide : le flux doit quand même être consommé.
        io::copy(&mut data, &mut io::sink())?;
        report.skipped.push(name.to_owned());
        return Ok(());
    };
    report.bytes += super::write_file(&target, &mut data, modified(entry))?;
    report.files += 1;
    Ok(())
}

pub(super) fn create(items: &[Item], dest: &Path, opts: &CreateOptions, progress: &dyn Progress) -> Result<()> {
    progress.total(items.iter().map(|it| it.size).sum());
    let mut writer = ArchiveWriter::create(dest).map_err(map_err)?;
    let mut methods: Vec<EncoderConfiguration> = Vec::new();
    if let Some(pw) = opts.password.as_deref() {
        methods.push(AesEncoderOptions::new(Password::new(pw)).into());
        // Comme 7-Zip avec « chiffrer les noms » : sans mot de passe, rien n'est lisible.
        writer.set_encrypt_header(true);
    }
    methods.push(match opts.level {
        0 => EncoderConfiguration::new(EncoderMethod::COPY),
        level => {
            let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
            Lzma2Options::from_level_mt(level.min(9), threads, 32 << 20).into()
        }
    });
    writer.set_content_methods(methods);

    let mut entries = Vec::new();
    let mut readers = Vec::new();
    for it in items {
        check(progress)?;
        let entry = ArchiveEntry::from_path(&it.path, it.name.clone());
        if it.is_dir {
            writer.push_archive_entry(entry, None::<&[u8]>).map_err(map_err)?;
        } else {
            entries.push(entry);
            readers.push(SourceReader::new(LazyFile::new(it, progress)));
        }
    }
    // Un seul bloc solide pour les fichiers : meilleur taux, comme 7-Zip par défaut.
    if !entries.is_empty() {
        writer.push_archive_entries(entries, readers).map_err(map_err)?;
    }
    writer.finish()?;
    Ok(())
}

/// Ouvre le fichier au moment de le lire : un bloc solide peut contenir des milliers
/// de fichiers, on n'en garde qu'un ouvert à la fois.
struct LazyFile<'a> {
    path: PathBuf,
    name: String,
    file: Option<File>,
    done: bool,
    progress: &'a dyn Progress,
}

impl<'a> LazyFile<'a> {
    fn new(item: &Item, progress: &'a dyn Progress) -> Self {
        Self { path: item.path.clone(), name: item.name.clone(), file: None, done: false, progress }
    }
}

impl Read for LazyFile<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if self.progress.is_cancelled() {
            return Err(io::Error::other(CancelledMarker));
        }
        let file = match &mut self.file {
            Some(f) => f,
            None => {
                self.progress.entry(&self.name);
                self.file.insert(File::open(&self.path)?)
            }
        };
        let n = file.read(buf)?;
        self.progress.advance(n as u64);
        if n == 0 {
            self.done = true;
            self.file = None;
        }
        Ok(n)
    }
}

pub(super) fn test(path: &Path, pw: Option<&str>, progress: &dyn Progress) -> Result<crate::TestReport> {
    let mut reader = open(path, pw)?;
    progress.total(reader.archive().files.iter().map(|f| f.size()).sum());
    let mut report = crate::TestReport::default();
    let result = reader.for_each_entries(|entry, data| {
        if progress.is_cancelled() {
            return Ok(false);
        }
        if entry.is_directory() || entry.is_anti_item() {
            return Ok(true);
        }
        progress.entry(entry.name());
        match io::copy(&mut Tracked::new(data, progress), &mut io::sink()) {
            Ok(n) => {
                report.files += 1;
                report.bytes += n;
            }
            Err(e) => report.errors.push(format!("{} : {e}", entry.name())),
        }
        Ok(true)
    });
    check(progress)?;
    match result.map_err(map_err) {
        Err(Error::Password) => Err(Error::Password),
        Err(e) => {
            report.errors.push(e.to_string());
            Ok(report)
        }
        Ok(()) => Ok(report),
    }
}
