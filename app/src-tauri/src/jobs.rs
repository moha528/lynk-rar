//! Opérations longues : annulation et remontée d'avancement vers l'interface.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use lynk_core::Progress;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct Jobs(Mutex<HashMap<u32, Arc<AtomicBool>>>);

impl Jobs {
    pub fn start(&self, id: u32) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.0.lock().unwrap().insert(id, flag.clone());
        flag
    }

    pub fn finish(&self, id: u32) {
        self.0.lock().unwrap().remove(&id);
    }

    pub fn cancel(&self, id: u32) {
        if let Some(flag) = self.0.lock().unwrap().get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    job: u32,
    done: u64,
    total: u64,
    current: String,
}

/// Toutes les 50 ms au plus : assez fluide pour l'œil, négligeable pour l'IPC.
const EMIT_EVERY: Duration = Duration::from_millis(50);

pub struct JobProgress {
    app: AppHandle,
    job: u32,
    cancel: Arc<AtomicBool>,
    total: AtomicU64,
    done: AtomicU64,
    current: Mutex<String>,
    last_emit: Mutex<Instant>,
}

impl JobProgress {
    pub fn new(app: AppHandle, job: u32, cancel: Arc<AtomicBool>) -> Self {
        Self {
            app,
            job,
            cancel,
            total: AtomicU64::new(0),
            done: AtomicU64::new(0),
            current: Mutex::new(String::new()),
            last_emit: Mutex::new(Instant::now() - EMIT_EVERY),
        }
    }

    fn emit(&self, force: bool) {
        {
            let mut last = self.last_emit.lock().unwrap();
            if !force && last.elapsed() < EMIT_EVERY {
                return;
            }
            *last = Instant::now();
        }
        let event = ProgressEvent {
            job: self.job,
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            current: self.current.lock().unwrap().clone(),
        };
        let _ = self.app.emit("job-progress", event);
    }

    /// Dernier état, envoyé quoi qu'il arrive en fin d'opération.
    pub fn flush(&self) {
        self.emit(true);
    }
}

impl Progress for JobProgress {
    fn total(&self, bytes: u64) {
        self.total.store(bytes, Ordering::Relaxed);
        self.emit(true);
    }

    fn entry(&self, name: &str) {
        name.clone_into(&mut self.current.lock().unwrap());
        self.emit(false);
    }

    fn advance(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
        self.emit(false);
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}
