//! Ligne de commande de l'application : c'est par elle que l'Explorateur
//! (menu contextuel, double-clic sur une archive) déclenche les actions.

use std::{ffi::OsString, path::PathBuf};

use serde::Serialize;
use winsar_core::Format;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// Fenêtre d'exploration, éventuellement sur une archive.
    Open,
    /// Extraction intelligente à côté de chaque archive.
    ExtractHere,
    /// Extraction dans un dossier au nom de chaque archive.
    ExtractNamed,
    /// Demande un dossier, puis extraction intelligente dedans.
    ExtractTo,
    /// Création immédiate de `<nom>.<format>` à côté des fichiers.
    Compress,
    /// Fenêtre de création avec les options.
    CompressDialog,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub action: Action,
    pub files: Vec<String>,
    pub format: Option<Format>,
}

impl Launch {
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Self {
        let mut action = Action::Open;
        let mut format = None;
        let mut files = Vec::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--extract-here") => action = Action::ExtractHere,
                Some("--extract-named") => action = Action::ExtractNamed,
                Some("--extract-to") => action = Action::ExtractTo,
                Some("--compress-dialog") => action = Action::CompressDialog,
                Some("--compress") => {
                    action = Action::Compress;
                    format = args.next().and_then(|f| Format::from_name(&format!(".{}", f.to_string_lossy())));
                }
                // Grande sélection depuis l'Explorateur : un chemin par ligne, fichier jetable.
                Some("--files-from") => {
                    if let Some(list) = args.next() {
                        if let Ok(text) = std::fs::read_to_string(&list) {
                            files.extend(text.lines().filter(|l| !l.is_empty()).map(str::to_owned));
                        }
                        let _ = std::fs::remove_file(list);
                    }
                }
                _ => {
                    let path = PathBuf::from(arg);
                    let path = std::path::absolute(&path).unwrap_or(path);
                    files.push(path.to_string_lossy().into_owned());
                }
            }
        }
        Self { action, files, format }
    }

    /// Une tâche s'affiche dans une petite fenêtre de progression, pas dans l'explorateur.
    pub fn is_task(&self) -> bool {
        !self.files.is_empty()
            && matches!(self.action, Action::ExtractHere | Action::ExtractNamed | Action::ExtractTo | Action::Compress)
    }
}
