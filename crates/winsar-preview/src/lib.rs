//! Aperçu du contenu d'une archive dans le volet de visualisation de l'Explorateur.
//!
//! L'Explorateur charge ce gestionnaire dans `prevhost.exe`, lui donne le fichier
//! (`IInitializeWithFile`) et une fenêtre hôte (`SetWindow`), puis demande
//! l'aperçu (`DoPreview`) : on y pose une liste Windows native.

#![cfg(windows)]
#![allow(non_snake_case)] // Noms imposés par les interfaces COM.

mod listing;

use std::{
    cell::RefCell,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr::null_mut,
    sync::atomic::{AtomicUsize, Ordering},
};

use windows::Win32::{
    Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, COLORREF, E_FAIL, E_NOTIMPL, E_POINTER, HWND, RECT, S_FALSE,
        S_OK,
    },
    Graphics::Gdi::LOGFONTW,
    System::{
        Com::{IClassFactory, IClassFactory_Impl},
        Ole::{IObjectWithSite, IObjectWithSite_Impl, IOleWindow, IOleWindow_Impl},
    },
    UI::{
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        Shell::{
            IPreviewHandler, IPreviewHandler_Impl, IPreviewHandlerFrame, IPreviewHandlerVisuals,
            IPreviewHandlerVisuals_Impl,
            PropertiesSystem::{IInitializeWithFile, IInitializeWithFile_Impl},
        },
        WindowsAndMessaging::{DestroyWindow, MSG, MoveWindow, SetParent},
    },
};
use windows_core::{BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR, Ref, Result, implement};

/// Doit correspondre à `DesktopPreviewHandler` dans `packaging/AppxManifest.xml`.
pub const CLSID_WINSAR_PREVIEW: GUID = GUID::from_u128(0x91de3b61_5509_4a65_be33_26284df4895c);

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

fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_FAIL.into()))
}

#[derive(Default)]
struct State {
    path: Option<PathBuf>,
    parent: HWND,
    rect: RECT,
    list: Option<HWND>,
    site: Option<IUnknown>,
    background: Option<COLORREF>,
    text: Option<COLORREF>,
}

#[implement(IPreviewHandler, IPreviewHandlerVisuals, IInitializeWithFile, IObjectWithSite, IOleWindow)]
struct Preview {
    state: RefCell<State>,
    _live: Live,
}

impl Preview_Impl {
    fn destroy_list(&self) {
        if let Some(list) = self.state.borrow_mut().list.take() {
            unsafe {
                let _ = DestroyWindow(list);
            }
        }
    }
}

fn place(list: HWND, r: &RECT) {
    unsafe {
        let _ = MoveWindow(list, r.left, r.top, r.right - r.left, r.bottom - r.top, true);
    }
    listing::fit_columns(list, r.right - r.left);
}

impl IPreviewHandler_Impl for Preview_Impl {
    fn SetWindow(&self, hwnd: HWND, prc: *const RECT) -> Result<()> {
        guard(|| {
            if prc.is_null() {
                return Err(E_POINTER.into());
            }
            let mut st = self.state.borrow_mut();
            st.parent = hwnd;
            st.rect = unsafe { *prc };
            if let Some(list) = st.list {
                unsafe { SetParent(list, Some(hwnd))? };
                place(list, &st.rect);
            }
            Ok(())
        })
    }

    fn SetRect(&self, prc: *const RECT) -> Result<()> {
        guard(|| {
            if prc.is_null() {
                return Err(E_POINTER.into());
            }
            let mut st = self.state.borrow_mut();
            st.rect = unsafe { *prc };
            if let Some(list) = st.list {
                place(list, &st.rect);
            }
            Ok(())
        })
    }

    fn DoPreview(&self) -> Result<()> {
        guard(|| {
            self.destroy_list();
            let mut st = self.state.borrow_mut();
            let path = st.path.clone().ok_or(E_FAIL)?;
            let list = listing::create(st.parent, &st.rect)?;
            listing::apply_colors(list, st.background, st.text);
            listing::fill(list, &path);
            st.list = Some(list);
            Ok(())
        })
    }

    fn Unload(&self) -> Result<()> {
        guard(|| {
            self.destroy_list();
            let mut st = self.state.borrow_mut();
            st.path = None;
            Ok(())
        })
    }

    fn SetFocus(&self) -> Result<()> {
        if let Some(list) = self.state.borrow().list {
            unsafe {
                let _ = SetFocus(Some(list));
            }
        }
        Ok(())
    }

    fn QueryFocus(&self) -> Result<HWND> {
        let focus = unsafe { GetFocus() };
        if focus.is_invalid() { Err(E_FAIL.into()) } else { Ok(focus) }
    }

    /// Les raccourcis non traités remontent à l'Explorateur (F5, Alt+…, etc.).
    fn TranslateAccelerator(&self, pmsg: *const MSG) -> Result<()> {
        let frame = self.state.borrow().site.as_ref().and_then(|s| s.cast::<IPreviewHandlerFrame>().ok());
        match frame {
            Some(frame) => unsafe { frame.TranslateAccelerator(pmsg) },
            None => Err(S_FALSE.into()),
        }
    }
}

impl IPreviewHandlerVisuals_Impl for Preview_Impl {
    fn SetBackgroundColor(&self, color: COLORREF) -> Result<()> {
        let mut st = self.state.borrow_mut();
        st.background = Some(color);
        if let Some(list) = st.list {
            listing::apply_colors(list, st.background, st.text);
        }
        Ok(())
    }

    fn SetFont(&self, _plf: *const LOGFONTW) -> Result<()> {
        Ok(()) // La police système de la liste convient.
    }

    fn SetTextColor(&self, color: COLORREF) -> Result<()> {
        let mut st = self.state.borrow_mut();
        st.text = Some(color);
        if let Some(list) = st.list {
            listing::apply_colors(list, st.background, st.text);
        }
        Ok(())
    }
}

impl IInitializeWithFile_Impl for Preview_Impl {
    fn Initialize(&self, pszfilepath: &PCWSTR, _grfmode: u32) -> Result<()> {
        guard(|| {
            let path = unsafe { pszfilepath.to_string() }.map_err(|_| E_FAIL)?;
            self.state.borrow_mut().path = Some(PathBuf::from(path));
            Ok(())
        })
    }
}

impl IObjectWithSite_Impl for Preview_Impl {
    fn SetSite(&self, punksite: Ref<IUnknown>) -> Result<()> {
        self.state.borrow_mut().site = punksite.cloned();
        Ok(())
    }

    fn GetSite(&self, riid: *const GUID, ppvsite: *mut *mut c_void) -> Result<()> {
        if ppvsite.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvsite = null_mut() };
        match &self.state.borrow().site {
            Some(site) => unsafe { site.query(riid, ppvsite) }.ok(),
            None => Err(E_FAIL.into()),
        }
    }
}

impl IOleWindow_Impl for Preview_Impl {
    fn GetWindow(&self) -> Result<HWND> {
        let st = self.state.borrow();
        Ok(st.list.unwrap_or(st.parent))
    }

    fn ContextSensitiveHelp(&self, _fentermode: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

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
        let preview: IPreviewHandler = Preview { state: RefCell::default(), _live: Live::new() }.into();
        unsafe { preview.query(riid, ppv) }.ok()
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

/// # Safety
/// Appelée par COM avec des pointeurs valides ou nuls.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if clsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = null_mut();
        if *clsid != CLSID_WINSAR_PREVIEW {
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
