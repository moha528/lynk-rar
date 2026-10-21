use std::{path::PathBuf, process::ExitCode, time::Duration};

use anyhow::{Context, bail};
use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use lynk_core::{CreateOptions, Error, ExtractOptions, Format, Overwrite, Progress};

#[derive(Parser)]
#[command(name = "lynk", version, about = "Lynk : des archives rapides et bien intégrées à Windows")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Liste le contenu d'une archive
    #[command(alias = "l")]
    List {
        archive: PathBuf,
        #[arg(short, long)]
        password: Option<String>,
        /// Sortie JSON (pour les scripts)
        #[arg(long)]
        json: bool,
    },
    /// Extrait une archive (par défaut : extraction intelligente à côté de l'archive)
    #[command(alias = "x")]
    Extract {
        archive: PathBuf,
        /// Dossier de destination
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long)]
        password: Option<String>,
        /// Fichiers déjà présents
        #[arg(long, value_enum, default_value_t = OverwriteArg::Rename)]
        overwrite: OverwriteArg,
    },
    /// Crée une archive à partir de fichiers et de dossiers
    #[command(alias = "a")]
    Create {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        /// Archive à créer (le format se déduit de l'extension)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// zip, 7z, tar, tar.gz, tar.bz2, tar.xz, tar.zst
        #[arg(short, long, value_parser = parse_format)]
        format: Option<Format>,
        /// 0 = stockage, 9 = maximum
        #[arg(short, long, default_value_t = 6, value_parser = clap::value_parser!(u32).range(0..=9))]
        level: u32,
        /// Chiffre l'archive (AES-256, ZIP et 7z)
        #[arg(short, long)]
        password: Option<String>,
        /// Découpe en volumes .001, .002… (ex. 700M, 4G, 100K)
        #[arg(short, long, value_parser = parse_size)]
        volume: Option<u64>,
    },
    /// Convertit une archive vers un autre format (RAR → 7z, ZIP → TAR.ZST…)
    #[command(alias = "c")]
    Convert {
        archive: PathBuf,
        /// Format cible : zip, 7z, tar, tar.gz, tar.bz2, tar.xz, tar.zst
        #[arg(short, long, value_parser = parse_format)]
        format: Option<Format>,
        /// Nouvelle archive (par défaut : à côté, au même nom)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 0 = stockage, 9 = maximum
        #[arg(short, long, default_value_t = 6, value_parser = clap::value_parser!(u32).range(0..=9))]
        level: u32,
        /// Mot de passe de l'archive d'origine
        #[arg(short, long)]
        password: Option<String>,
        /// Chiffre la nouvelle archive (AES-256, ZIP et 7z)
        #[arg(long)]
        new_password: Option<String>,
        /// Découpe en volumes .001, .002… (ex. 700M, 4G, 100K)
        #[arg(short, long, value_parser = parse_size)]
        volume: Option<u64>,
    },
    /// Vérifie l'intégrité d'une archive sans rien extraire
    #[command(alias = "t")]
    Test {
        archive: PathBuf,
        #[arg(short, long)]
        password: Option<String>,
    },
    /// Formats pris en charge
    Formats,
}

/// `700M`, `4G`, `100K`, `1048576` : en octets, unités binaires.
fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (num, mult) = match s.chars().last().map(|c| c.to_ascii_uppercase()) {
        Some('K') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M') => (&s[..s.len() - 1], 1 << 20),
        Some('G') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    match num.trim().parse::<u64>() {
        Ok(n) if n > 0 => Ok(n * mult),
        _ => Err(format!("taille invalide : {s}")),
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum OverwriteArg {
    Skip,
    Replace,
    Rename,
}

impl From<OverwriteArg> for Overwrite {
    fn from(o: OverwriteArg) -> Self {
        match o {
            OverwriteArg::Skip => Overwrite::Skip,
            OverwriteArg::Replace => Overwrite::Replace,
            OverwriteArg::Rename => Overwrite::Rename,
        }
    }
}

fn parse_format(s: &str) -> Result<Format, String> {
    Format::from_name(&format!(".{}", s.trim_start_matches('.'))).ok_or_else(|| format!("format inconnu : {s}"))
}

struct Bar(ProgressBar);

impl Bar {
    fn new() -> Self {
        let bar = ProgressBar::new(0);
        bar.set_style(
            ProgressStyle::with_template("{spinner} [{bar:32}] {bytes}/{total_bytes} {bytes_per_sec} {wide_msg}")
                .expect("gabarit valide")
                .progress_chars("=> "),
        );
        bar.enable_steady_tick(Duration::from_millis(120));
        Self(bar)
    }
}

impl Progress for Bar {
    fn total(&self, bytes: u64) {
        self.0.set_length(bytes);
    }
    fn entry(&self, name: &str) {
        self.0.set_message(name.to_owned());
    }
    fn advance(&self, bytes: u64) {
        self.0.inc(bytes);
    }
}

/// Relance l'opération en demandant le mot de passe s'il manque ou s'il est faux.
fn with_password<T>(
    mut password: Option<String>,
    mut op: impl FnMut(Option<String>) -> lynk_core::Result<T>,
) -> anyhow::Result<T> {
    for _ in 0..3 {
        match op(password.clone()) {
            Err(Error::Password) => {
                password = Some(rpassword::prompt_password("Mot de passe : ")?);
            }
            other => return Ok(other?),
        }
    }
    bail!("mot de passe incorrect")
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} o") } else { format!("{v:.1} {}", UNITS[unit]) }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.cmd {
        Cmd::List { archive, password, json } => {
            let entries = with_password(password, |pw| lynk_core::list(&archive, pw.as_deref()))
                .with_context(|| archive.display().to_string())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }
            for e in &entries {
                let date = e
                    .modified
                    .and_then(|t| Local.timestamp_opt(t, 0).single())
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                let size = if e.is_dir { "<dossier>".to_owned() } else { human(e.size) };
                let lock = if e.encrypted { "*" } else { " " };
                println!("{size:>10}  {date:16} {lock} {}", e.path);
            }
            let total: u64 = entries.iter().map(|e| e.size).sum();
            println!("{} entrées, {}", entries.len(), human(total));
        }
        Cmd::Extract { archive, output, password, overwrite } => {
            let bar = Bar::new();
            let report = with_password(password, |pw| {
                let opts = ExtractOptions { password: pw, overwrite: overwrite.into(), ..Default::default() };
                bar.0.reset();
                match &output {
                    Some(dest) => lynk_core::extract(&archive, dest, &opts, &bar),
                    None => lynk_core::extract_smart(&archive, &opts, &bar),
                }
            })
            .with_context(|| archive.display().to_string())?;
            bar.0.finish_and_clear();
            println!(
                "{} fichiers, {} dossiers, {} → {}",
                report.files,
                report.dirs,
                human(report.bytes),
                report.destination.display()
            );
            for s in &report.skipped {
                eprintln!("ignoré : {s}");
            }
        }
        Cmd::Create { sources, output, format, level, password, volume } => {
            let format = format
                .or_else(|| output.as_ref().and_then(|o| Format::from_name(&o.to_string_lossy())))
                .unwrap_or(Format::Zip);
            let dest = output.unwrap_or_else(|| lynk_core::default_archive_path(&sources, format));
            let opts = CreateOptions { format, level, password, volume_size: volume };
            let bar = Bar::new();
            let parts = lynk_core::create(&sources, &dest, &opts, &bar)
                .with_context(|| format!("création de {}", dest.display()))?;
            bar.0.finish_and_clear();
            for part in &parts {
                let size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
                println!("{} ({})", part.display(), human(size));
            }
        }
        Cmd::Convert { archive, format, output, level, password, new_password, volume } => {
            let Some(format) = format.or_else(|| output.as_ref().and_then(|o| Format::from_name(&o.to_string_lossy())))
            else {
                bail!("précisez le format cible (--format 7z) ou une sortie avec son extension");
            };
            let dest =
                output.unwrap_or_else(|| lynk_core::default_archive_path(std::slice::from_ref(&archive), format));
            let create = CreateOptions { format, level, password: new_password, volume_size: volume };
            let bar = Bar::new();
            let parts = with_password(password, |pw| {
                bar.0.reset();
                let opts = lynk_core::ConvertOptions { password: pw, create: create.clone() };
                lynk_core::convert(&archive, &dest, &opts, &bar)
            })
            .with_context(|| format!("conversion de {}", archive.display()))?;
            bar.0.finish_and_clear();
            for part in &parts {
                let size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
                println!("{} ({})", part.display(), human(size));
            }
        }
        Cmd::Test { archive, password } => {
            let bar = Bar::new();
            let report = with_password(password, |pw| {
                bar.0.reset();
                lynk_core::test(&archive, pw.as_deref(), &bar)
            })
            .with_context(|| archive.display().to_string())?;
            bar.0.finish_and_clear();
            for e in &report.errors {
                eprintln!("erreur : {e}");
            }
            if !report.is_ok() {
                bail!("{} erreur(s) dans {}", report.errors.len(), archive.display());
            }
            println!("Archive intacte : {} fichiers, {} vérifiés", report.files, human(report.bytes));
        }
        Cmd::Formats => {
            for f in Format::ALL {
                let mode = if f.can_create() { "lecture + écriture" } else { "lecture" };
                let crypt = if f.can_encrypt() { ", chiffrement" } else { "" };
                println!("{:8} .{:8} {mode}{crypt}", f.label(), f.extension());
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e:#}");
            ExitCode::FAILURE
        }
    }
}
