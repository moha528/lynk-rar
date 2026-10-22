use serde::Serialize;

/// Une entrée (fichier ou dossier) d'une archive, telle que listée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Chemin dans l'archive, séparé par `/`, sans `/` final.
    pub path: String,
    pub size: u64,
    pub packed_size: Option<u64>,
    pub is_dir: bool,
    /// Date de modification, en secondes Unix.
    pub modified: Option<i64>,
    pub encrypted: bool,
}

impl Entry {
    pub(crate) fn new(path: &str, is_dir: bool, size: u64) -> Self {
        Self {
            path: path.replace('\\', "/").trim_end_matches('/').to_owned(),
            size,
            packed_size: None,
            is_dir,
            modified: None,
            encrypted: false,
        }
    }
}
