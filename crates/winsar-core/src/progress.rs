use std::{
    fmt,
    io::{self, Read},
    sync::Mutex,
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

/// Deux étapes enchaînées (extraction puis recompression) vues comme une seule
/// barre : chacune en compte la moitié, sans que la barre ne reparte de zéro.
pub(crate) struct TwoSteps<'a> {
    inner: &'a dyn Progress,
    state: Mutex<StepState>,
}

#[derive(Default)]
struct StepState {
    second: bool,
    first_total: u64,
    second_total: u64,
    second_done: u64,
    /// Octets déjà remontés pour la seconde étape, ramenés à l'échelle de la première.
    second_reported: u64,
}

impl<'a> TwoSteps<'a> {
    pub(crate) fn new(inner: &'a dyn Progress) -> Self {
        Self { inner, state: Mutex::new(StepState::default()) }
    }

    /// Passe à la seconde étape.
    pub(crate) fn next(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).second = true;
    }
}

impl Progress for TwoSteps<'_> {
    fn total(&self, bytes: u64) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.second {
            s.second_total = bytes;
        } else {
            s.first_total = bytes;
            self.inner.total(bytes.saturating_mul(2));
        }
    }

    fn entry(&self, name: &str) {
        self.inner.entry(name);
    }

    fn advance(&self, bytes: u64) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.second {
            self.inner.advance(bytes);
            return;
        }
        s.second_done += bytes;
        let scaled = match s.second_total {
            0 => 0,
            total => (u128::from(s.second_done) * u128::from(s.first_total) / u128::from(total)) as u64,
        }
        .min(s.first_total);
        if scaled > s.second_reported {
            self.inner.advance(scaled - s.second_reported);
            s.second_reported = scaled;
        }
    }

    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

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
