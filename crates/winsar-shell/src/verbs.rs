//! Les entrées du sous-menu : libellés, visibilité et ligne de commande de winsar-app.
//! Sans dépendance à COM, donc testable normalement.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    ExtractHere,
    ExtractNamed,
    ExtractTo,
    Open,
    Separator,
    CompressZip,
    Compress7z,
    CompressDialog,
}

/// Mêmes suffixes que `winsar_core::Format`, du plus long au plus court.
const ARCHIVE_SUFFIXES: &[&str] = &[
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".tbz", ".txz", ".tzst", ".tar", ".zip", ".jar",
    ".7z", ".rar",
];

fn archive_suffix(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    ARCHIVE_SUFFIXES.iter().copied().find(|s| lower.ends_with(s) && lower.len() > s.len())
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `photos.tar.gz` → `photos`, `film.part01.rar` → `film`.
fn archive_stem(p: &Path) -> String {
    let name = file_name(p);
    let mut cut = archive_suffix(&name).map_or(name.len(), |s| name.len() - s.len());
    let lower = name.to_ascii_lowercase();
    if let Some(i) = lower[..cut].rfind(".part") {
        let digits = &lower[i + 5..cut];
        if i > 0 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            cut = i;
        }
    }
    name[..cut].to_owned()
}

pub struct Selection {
    pub paths: Vec<PathBuf>,
    archives: usize,
}

impl Selection {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let archives = paths.iter().filter(|p| archive_suffix(&file_name(p)).is_some()).count();
        Self { paths, archives }
    }

    fn all_archives(&self) -> bool {
        self.archives > 0 && self.archives == self.paths.len()
    }

    /// Nom proposé par winsar-app pour « Ajouter à » (voir `winsar_core::default_archive_path`).
    fn archive_base(&self) -> String {
        let base = match self.paths.as_slice() {
            [one] if one.is_dir() => file_name(one),
            [one] if archive_suffix(&file_name(one)).is_some() => archive_stem(one),
            [one] => one.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            [first, ..] => first.parent().map(file_name).unwrap_or_default(),
            [] => String::new(),
        };
        if base.is_empty() { "archive".to_owned() } else { base }
    }
}

impl Verb {
    pub const ALL: [Verb; 8] = [
        Verb::ExtractHere,
        Verb::ExtractNamed,
        Verb::ExtractTo,
        Verb::Open,
        Verb::Separator,
        Verb::CompressZip,
        Verb::Compress7z,
        Verb::CompressDialog,
    ];

    fn bit(self) -> u32 {
        1 << Verb::ALL.iter().position(|&v| v == self).unwrap_or(0)
    }

    /// `hidden` : entrées masquées dans les réglages de WinSAR (bit n = n-ième de `ALL`).
    pub fn visible(self, sel: &Selection, hidden: u32) -> bool {
        let shown = |v: Verb| hidden & v.bit() == 0 && v.applies(sel);
        match self {
            // Le séparateur n'a de sens qu'entre deux groupes visibles.
            Verb::Separator => {
                let extract = [Verb::ExtractHere, Verb::ExtractNamed, Verb::ExtractTo, Verb::Open];
                let compress = [Verb::CompressZip, Verb::Compress7z, Verb::CompressDialog];
                extract.into_iter().any(shown) && compress.into_iter().any(shown)
            }
            v => shown(v),
        }
    }

    fn applies(self, sel: &Selection) -> bool {
        match self {
            Verb::ExtractHere | Verb::ExtractNamed | Verb::ExtractTo | Verb::Separator => sel.all_archives(),
            Verb::Open => sel.all_archives() && sel.paths.len() == 1,
            Verb::CompressZip | Verb::Compress7z | Verb::CompressDialog => true,
        }
    }

    pub fn title(self, sel: &Selection) -> String {
        match self {
            Verb::ExtractHere => "Extraire ici".into(),
            Verb::ExtractNamed => match sel.paths.as_slice() {
                [one] => format!("Extraire dans « {}\\ »", archive_stem(one)),
                _ => "Extraire chaque archive dans son dossier".into(),
            },
            Verb::ExtractTo => "Extraire vers…".into(),
            Verb::Open => "Ouvrir avec WinSAR".into(),
            Verb::Separator => String::new(),
            Verb::CompressZip => format!("Ajouter à « {}.zip »", sel.archive_base()),
            Verb::Compress7z => format!("Ajouter à « {}.7z »", sel.archive_base()),
            Verb::CompressDialog => "Ajouter à une archive…".into(),
        }
    }

    /// Contrat avec `app/src-tauri/src/launch.rs`.
    fn args(self) -> &'static [&'static str] {
        match self {
            Verb::ExtractHere => &["--extract-here"],
            Verb::ExtractNamed => &["--extract-named"],
            Verb::ExtractTo => &["--extract-to"],
            Verb::Open | Verb::Separator => &[],
            Verb::CompressZip => &["--compress", "zip"],
            Verb::Compress7z => &["--compress", "7z"],
            Verb::CompressDialog => &["--compress-dialog"],
        }
    }
}

/// Au-delà, la ligne de commande Windows (32 767 caractères) serait trop courte :
/// la liste passe par un fichier temporaire que winsar-app lit puis supprime.
const MAX_INLINE_CHARS: usize = 24_000;

pub fn command(exe: &Path, verb: Verb, sel: &Selection) -> io::Result<Command> {
    let mut cmd = Command::new(exe);
    cmd.args(verb.args());
    let length: usize = sel.paths.iter().map(|p| p.as_os_str().len() + 3).sum();
    if length > MAX_INLINE_CHARS {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let list = std::env::temp_dir().join(format!("winsar-{}-{stamp}.txt", std::process::id()));
        let text: Vec<String> = sel.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        std::fs::write(&list, text.join("\n"))?;
        cmd.arg("--files-from").arg(list);
    } else {
        cmd.args(&sel.paths);
    }
    Ok(cmd)
}

pub fn launch(exe: &Path, verb: Verb, sel: &Selection) -> io::Result<()> {
    command(exe, verb, sel)?.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(paths: &[&str]) -> Selection {
        Selection::new(paths.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn archives_get_extract_verbs() {
        let one = sel(&[r"C:\d\photos.tar.gz"]);
        assert!(Verb::ExtractHere.visible(&one, 0) && Verb::Open.visible(&one, 0));
        assert_eq!(Verb::ExtractNamed.title(&one), "Extraire dans « photos\\ »");

        let mixed = sel(&[r"C:\d\a.zip", r"C:\d\notes.txt"]);
        assert!(!Verb::ExtractHere.visible(&mixed, 0) && !Verb::Separator.visible(&mixed, 0));
        assert!(Verb::CompressZip.visible(&mixed, 0));
        assert_eq!(Verb::Compress7z.title(&mixed), "Ajouter à « d.7z »");

        let many = sel(&[r"C:\d\a.zip", r"C:\d\film.part01.rar"]);
        assert!(Verb::ExtractHere.visible(&many, 0) && !Verb::Open.visible(&many, 0));
        assert_eq!(archive_stem(Path::new("film.part01.rar")), "film");
    }

    #[test]
    fn hidden_entries_and_separator() {
        let one = sel(&[r"C:\d\a.zip"]);
        let hide_zip_7z = Verb::CompressZip.bit() | Verb::Compress7z.bit();
        assert!(!Verb::CompressZip.visible(&one, hide_zip_7z));
        assert!(Verb::Separator.visible(&one, hide_zip_7z), "il reste « Ajouter à une archive… »");
        let all_compress = hide_zip_7z | Verb::CompressDialog.bit();
        assert!(!Verb::Separator.visible(&one, all_compress));
    }

    #[test]
    fn compress_title_uses_file_stem() {
        assert_eq!(Verb::CompressZip.title(&sel(&[r"C:\d\rapport.pdf"])), "Ajouter à « rapport.zip »");
    }

    #[test]
    fn long_selections_go_through_a_file() {
        let paths: Vec<String> = (0..2000).map(|i| format!(r"C:\un\chemin\assez\long\fichier-{i}.txt")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let cmd = command(Path::new("winsar-app.exe"), Verb::CompressZip, &sel(&refs)).unwrap();
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[..3], ["--compress", "zip", "--files-from"]);
        let list = PathBuf::from(&args[3]);
        assert_eq!(std::fs::read_to_string(&list).unwrap().lines().count(), 2000);
        std::fs::remove_file(list).unwrap();
    }
}
