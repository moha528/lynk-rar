use std::{io, path::PathBuf};

use crate::{Format, progress::CancelledMarker};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("erreur d'entrée/sortie : {0}")]
    Io(io::Error),
    #[error("format d'archive non reconnu : {}", .0.display())]
    UnknownFormat(PathBuf),
    #[error("le format {} est en lecture seule", .0.label())]
    CannotCreate(Format),
    #[error("non pris en charge : {0}")]
    Unsupported(&'static str),
    #[error("mot de passe requis ou incorrect")]
    Password,
    #[error("opération annulée")]
    Cancelled,
    #[error("archive corrompue ou illisible : {0}")]
    Archive(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        // Un lecteur suivi signale l'annulation par une erreur d'E/S marquée.
        if e.get_ref().is_some_and(|inner| inner.is::<CancelledMarker>()) { Error::Cancelled } else { Error::Io(e) }
    }
}

impl From<walkdir::Error> for Error {
    fn from(e: walkdir::Error) -> Self {
        Error::Io(e.into())
    }
}
