//! Menu contextuel natif de Windows 11 pour WinSAR (`IExplorerCommand`).
//!
//! Cette DLL est chargée dans `explorer.exe` : elle ne fait que calculer les
//! libellés et lancer `winsar-app.exe` (posé à côté d'elle) avec les bons arguments.
//! Aucune panique ne doit s'en échapper, chaque point d'entrée est gardé.

#![cfg(windows)]
#![allow(non_snake_case)] // Noms imposés par les interfaces COM.

mod verbs;

use std::{
    ffi::c_void,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
};

use windows::Win32::{
    Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_POINTER, HINSTANCE, HMODULE, S_FALSE,
        S_OK,
    },
    System::{
        Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl},
        LibraryLoader::GetModuleFileNameW,
        SystemServices::DLL_PROCESS_ATTACH,
    },
    UI::Shell::{
        ECF_DEFAULT, ECF_HASSUBCOMMANDS, ECF_ISSEPARATOR, ECS_ENABLED, ECS_HIDDEN, IEnumExplorerCommand,
        IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray, SHStrDupW,
        SIGDN_FILESYSPATH,
    },
};
use windows_core::{BOOL, GUID, HRESULT, HSTRING, IUnknown, Interface, PWSTR, Ref, Result, implement};

use verbs::{Selection, Verb};

/// Doit correspondre au `Clsid` déclaré dans `packaging/AppxManifest.xml`.
pub const CLSID_WINSAR: GUID = GUID::from_u128(0xc3c32fbf_e75b_4ec3_a8aa_b6e081c97d8a);

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// Objets COM vivants : tant qu'il en reste, la DLL ne doit pas être déchargée.
static LIVE: AtomicUsize = AtomicUsize::new(0);

struct Live;

impl Live {
    fn new() -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Live
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Une panique dans explorer.exe le ferait tomber : on la convertit en E_FAIL.
fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_FAIL.into()))
}

fn dup(text: &str) -> Result<PWSTR> {
    unsafe { SHStrDupW(&HSTRING::from(text)) }
}

/// `winsar-app.exe` est installé dans le même dossier que cette DLL.
fn app_exe() -> Option<PathBuf> {
    let mut buf = [0u16; 2048];
    let module = HMODULE(MODULE.load(Ordering::Relaxed));
    let len = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])).with_file_name("winsar-app.exe"))
}

/// Chemins des éléments sélectionnés ; `None` si l'un d'eux n'est pas un vrai
/// fichier (corbeille, panneau de configuration, appareil mobile…).
fn selection(items: Ref<IShellItemArray>) -> Option<Selection> {
    let items = items.as_ref()?;
    let count = unsafe { items.GetCount() }.ok()?;
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        let item = unsafe { items.GetItemAt(i) }.ok()?;
        let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
        let path = unsafe { name.to_string() };
        unsafe { CoTaskMemFree(Some(name.0 as *const c_void)) };
        paths.push(PathBuf::from(path.ok()?));
    }
    (!paths.is_empty()).then(|| Selection::new(paths))
}

/// Entrées masquées dans les réglages de WinSAR (`HKCU\Software\WinSAR\ContextMenuHidden`).
fn menu_hidden() -> u32 {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows_core::w!(r"Software\WinSAR"),
            windows_core::w!("ContextMenuHidden"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    if status.is_ok() { value } else { 0 }
}

fn state(visible: bool) -> u32 {
    (if visible { ECS_ENABLED } else { ECS_HIDDEN }).0 as u32
}

// ——— Entrée racine « WinSAR », qui porte le sous-menu ———

#[implement(IExplorerCommand)]
struct Root {
    _live: Live,
}

impl IExplorerCommand_Impl for Root_Impl {
    fn GetTitle(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        dup("WinSAR")
    }

    fn GetIcon(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        guard(|| {
            let exe = app_exe().ok_or(E_FAIL)?;
            dup(&format!("{},0", exe.display()))
        })
    }

    fn GetToolTip(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(CLSID_WINSAR)
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> Result<u32> {
        guard(|| Ok(state(selection(items).is_some())))
    }

    fn Invoke(&self, _: Ref<IShellItemArray>, _: Ref<IBindCtx>) -> Result<()> {
        Ok(())
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_HASSUBCOMMANDS.0 as u32)
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        guard(|| {
            let items =
                Verb::ALL.iter().map(|&verb| IExplorerCommand::from(Sub { verb, _live: Live::new() })).collect();
            Ok(SubCommands { items, pos: AtomicUsize::new(0), _live: Live::new() }.into())
        })
    }
}

// ——— Une entrée du sous-menu ———

#[implement(IExplorerCommand)]
struct Sub {
    verb: Verb,
    _live: Live,
}

impl IExplorerCommand_Impl for Sub_Impl {
    fn GetTitle(&self, items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guard(|| {
            let sel = selection(items).ok_or(E_FAIL)?;
            dup(&self.verb.title(&sel))
        })
    }

    fn GetIcon(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Err(E_NOTIMPL.into())
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> Result<u32> {
        guard(|| Ok(state(selection(items).is_some_and(|sel| self.verb.visible(&sel, menu_hidden())))))
    }

    fn Invoke(&self, items: Ref<IShellItemArray>, _: Ref<IBindCtx>) -> Result<()> {
        guard(|| {
            let sel = selection(items).ok_or(E_FAIL)?;
            let exe = app_exe().ok_or(E_FAIL)?;
            verbs::launch(&exe, self.verb, &sel).map_err(|_| E_FAIL.into())
        })
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(if self.verb == Verb::Separator { ECF_ISSEPARATOR } else { ECF_DEFAULT }.0 as u32)
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

// ——— Énumérateur du sous-menu ———

#[implement(IEnumExplorerCommand)]
struct SubCommands {
    items: Vec<IExplorerCommand>,
    pos: AtomicUsize,
    _live: Live,
}

impl IEnumExplorerCommand_Impl for SubCommands_Impl {
    fn Next(&self, celt: u32, out: *mut Option<IExplorerCommand>, fetched: *mut u32) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        let start = self.pos.load(Ordering::Relaxed).min(self.items.len());
        let n = (celt as usize).min(self.items.len() - start);
        for (i, item) in self.items[start..start + n].iter().enumerate() {
            // Mémoire non initialisée côté appelant : écrire sans lire l'ancienne valeur.
            unsafe { out.add(i).write(Some(item.clone())) };
        }
        self.pos.store(start + n, Ordering::Relaxed);
        if !fetched.is_null() {
            unsafe { *fetched = n as u32 };
        }
        if n == celt as usize { S_OK } else { S_FALSE }
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        let pos = self.pos.load(Ordering::Relaxed).saturating_add(celt as usize);
        self.pos.store(pos.min(self.items.len()), Ordering::Relaxed);
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.pos.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        Ok(SubCommands {
            items: self.items.clone(),
            pos: AtomicUsize::new(self.pos.load(Ordering::Relaxed)),
            _live: Live::new(),
        }
        .into())
    }
}

// ——— Fabrique et exports COM ———

#[implement(IClassFactory)]
struct Factory {
    _live: Live,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        if ppv.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = null_mut() };
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let command: IExplorerCommand = Root { _live: Live::new() }.into();
        unsafe { command.query(riid, ppv) }.ok()
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            LIVE.fetch_add(1, Ordering::SeqCst);
        } else {
            LIVE.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module.0, Ordering::Relaxed);
    }
    true.into()
}

/// # Safety
/// Appelée par COM avec des pointeurs valides ou nuls.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if clsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = null_mut();
        if *clsid != CLSID_WINSAR {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
    }
    let factory: IClassFactory = Factory { _live: Live::new() }.into();
    unsafe { factory.query(riid, ppv) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
}
