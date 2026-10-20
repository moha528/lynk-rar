use std::{
    fmt,
    io::{self, Read},
};

use crate::{Error, Result};

/// Retour d'avancement d'une opération. Toutes les méthodes sont optionnelles.
///
/// `Sync` permet de piloter l'opération depuis un thread de travail tout en
/// lisant l'état (annulation, octets traités) depuis l'interface.
pub trait Progress: Sync {
    /// Volume total attendu, en octets.
    fn total(&self, _bytes: u64) {}
    /// Début du traitement d'une entrée.
    fn entry(&self, _name: &str) {}
    /// Octets traités depuis le dernier appel.
    fn advance(&self, _bytes: u64) {}
    /// Demande d'annulation, interrogée régulièrement.
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub struct NoProgress;

impl Progress for NoProgress {}

pub(crate) fn check(progress: &dyn Progress) -> Result<()> {
    if progress.is_cancelled() { Err(Error::Cancelled) } else { Ok(()) }
}

#[derive(Debug)]
pub(crate) struct CancelledMarker;

impl fmt::Display for CancelledMarker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("opération annulée")
    }
}

impl std::error::Error for CancelledMarker {}

/// Lecteur qui remonte les octets lus et s'interrompt dès qu'une annulation est demandée.
pub(crate) struct Tracked<'a, R> {
    inner: R,
    progress: &'a dyn Progress,
}

impl<'a, R> Tracked<'a, R> {
    pub(crate) fn new(inner: R, progress: &'a dyn Progress) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read> Read for Tracked<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.progress.is_cancelled() {
            return Err(io::Error::other(CancelledMarker));
        }
        let n = self.inner.read(buf)?;
        self.progress.advance(n as u64);
        Ok(n)
    }
}
