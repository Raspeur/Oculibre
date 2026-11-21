//! The camera filter: the object apps create from our CLSID (`IBaseFilter`).
//!
//! Written by hand instead of with `#[implement]`: `GetState` must answer `VFW_S_CANT_CUE` (a
//! *success* code meaning "I am a live source: do not wait for a picture while paused") *and*
//! give the state, which the generated wrapper cannot do (it writes the state only with `S_OK`).
//! Without it, an app waiting for the paused state would hang.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{E_NOINTERFACE, E_NOTIMPL, E_POINTER, S_OK};
use windows::Win32::Media::DirectShow::*;
use windows::Win32::Media::IReferenceClock;
use windows::Win32::System::Com::{IPersist, IPersist_Vtbl};
use windows_core::{ComObject, IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT, PCWSTR, PWSTR};

use crate::com::{copy_name, guard_hr, wide};
use crate::enums::PinEnum;
use crate::pin::{Pin, PIN_NAME};
use crate::CLSID_OCULIBRE;

/// What the pin needs from its filter. Pointers without a reference, as DirectShow wants: a
/// filter must not keep its graph alive, and the pin must not keep its filter alive.
pub struct FilterShared {
    inner: Mutex<Inner>,
}

struct Inner {
    state: FILTER_STATE,
    /// Our filter object (null once it is gone).
    filter: *mut c_void,
    /// The graph we joined (not AddRef'd).
    graph: *mut c_void,
    name: Vec<u16>,
    clock: Option<IReferenceClock>,
}

// SAFETY: raw pointers to free-threaded COM objects, used under the mutex.
unsafe impl Send for Inner {}

impl FilterShared {
    pub fn stopped(&self) -> bool {
        self.inner.lock().unwrap().state == State_Stopped
    }

    /// Our filter, AddRef'd (QueryPinInfo).
    pub fn filter(&self) -> Option<IBaseFilter> {
        let raw = self.inner.lock().unwrap().filter;
        // SAFETY: a live filter (set to null in its last Release, under the same lock).
        unsafe { IBaseFilter::from_raw_borrowed(&raw).cloned() }
    }

    /// The graph, AddRef'd for the duration of a call.
    pub fn graph(&self) -> Option<IFilterGraph> {
        let raw = self.inner.lock().unwrap().graph;
        // SAFETY: the graph outlives its filters (it calls JoinFilterGraph(null) first).
        unsafe { IFilterGraph::from_raw_borrowed(&raw).cloned() }
    }
}

#[repr(C)]
struct Filter {
    vtable: &'static IBaseFilter_Vtbl,
    refs: AtomicU32,
    shared: Arc<FilterShared>,
    pin: ComObject<Pin>,
}

/// A new camera filter, as `IUnknown`.
pub fn create() -> IUnknown {
    let shared = Arc::new(FilterShared {
        inner: Mutex::new(Inner {
            state: State_Stopped,
            filter: std::ptr::null_mut(),
            graph: std::ptr::null_mut(),
            name: wide("Oculibre"),
            clock: None,
        }),
    });
    let filter = Box::into_raw(Box::new(Filter {
        vtable: &VTABLE,
        refs: AtomicU32::new(1),
        shared: shared.clone(),
        pin: ComObject::new(Pin::new(shared.clone())),
    }));
    shared.inner.lock().unwrap().filter = filter.cast();
    // SAFETY: a COM object whose first field is its vtable, with one reference for the caller.
    unsafe { IUnknown::from_raw(filter.cast()) }
}

/// # Safety
/// `this` is one of our filters.
unsafe fn this<'a>(this: *mut c_void) -> &'a Filter {
    // SAFETY: the caller's contract.
    unsafe { &*(this as *const Filter) }
}

static VTABLE: IBaseFilter_Vtbl = IBaseFilter_Vtbl {
    base__: IMediaFilter_Vtbl {
        base__: IPersist_Vtbl {
            base__: IUnknown_Vtbl {
                QueryInterface: query_interface,
                AddRef: add_ref,
                Release: release,
            },
            GetClassID: get_class_id,
        },
        Stop: stop,
        Pause: pause,
        Run: run,
        GetState: get_state,
        SetSyncSource: set_sync_source,
        GetSyncSource: get_sync_source,
    },
    EnumPins: enum_pins,
    FindPin: find_pin,
    QueryFilterInfo: query_filter_info,
    JoinFilterGraph: join_filter_graph,
    QueryVendorInfo: query_vendor_info,
};

unsafe extern "system" fn query_interface(
    me: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() || iid.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers, checked.
    unsafe {
        let iid = &*iid;
        if [
            IUnknown::IID,
            IPersist::IID,
            IMediaFilter::IID,
            IBaseFilter::IID,
        ]
        .contains(iid)
        {
            add_ref(me);
            out.write(me);
            S_OK
        } else {
            out.write(std::ptr::null_mut());
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn add_ref(me: *mut c_void) -> u32 {
    // SAFETY: one of ours.
    unsafe { this(me) }.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn release(me: *mut c_void) -> u32 {
    // SAFETY: one of ours.
    let filter = unsafe { this(me) };
    let left = filter.refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if left == 0 {
        filter.pin.inactive();
        filter.shared.inner.lock().unwrap().filter = std::ptr::null_mut();
        // SAFETY: the last reference: the box we made in `create`.
        drop(unsafe { Box::from_raw(me as *mut Filter) });
    }
    left
}

unsafe extern "system" fn get_class_id(_: *mut c_void, out: *mut GUID) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's GUID.
    unsafe { out.write(CLSID_OCULIBRE) };
    S_OK
}

unsafe extern "system" fn stop(me: *mut c_void) -> HRESULT {
    guard_hr(|| {
        // SAFETY: one of ours.
        let filter = unsafe { this(me) };
        filter.pin.inactive();
        filter.shared.inner.lock().unwrap().state = State_Stopped;
        S_OK
    })
}

unsafe extern "system" fn pause(me: *mut c_void) -> HRESULT {
    guard_hr(|| {
        // SAFETY: one of ours.
        let filter = unsafe { this(me) };
        let was = filter.shared.inner.lock().unwrap().state;
        if was == State_Stopped {
            if let Err(e) = filter.pin.active() {
                return e.code();
            }
        } else {
            filter.pin.pause();
        }
        filter.shared.inner.lock().unwrap().state = State_Paused;
        S_OK
    })
}

unsafe extern "system" fn run(me: *mut c_void, start: i64) -> HRESULT {
    guard_hr(|| {
        // SAFETY: one of ours.
        let filter = unsafe { this(me) };
        if filter.shared.inner.lock().unwrap().state == State_Stopped {
            // SAFETY: same object.
            let hr = unsafe { pause(me) };
            if hr.is_err() {
                return hr;
            }
        }
        let clock = filter.shared.inner.lock().unwrap().clock.clone();
        filter.pin.run(start, clock);
        filter.shared.inner.lock().unwrap().state = State_Running;
        S_OK
    })
}

unsafe extern "system" fn get_state(
    me: *mut c_void,
    _timeout: u32,
    out: *mut FILTER_STATE,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: one of ours; the caller's state.
    let state = unsafe { this(me) }.shared.inner.lock().unwrap().state;
    unsafe { out.write(state) };
    if state == State_Paused {
        VFW_S_CANT_CUE // live source: no picture to wait for while paused
    } else {
        S_OK
    }
}

unsafe extern "system" fn set_sync_source(me: *mut c_void, clock: *mut c_void) -> HRESULT {
    // SAFETY: one of ours; `clock` is a reference clock or null, borrowed (we AddRef our copy).
    let clock = unsafe { IReferenceClock::from_raw_borrowed(&clock).cloned() };
    unsafe { this(me) }.shared.inner.lock().unwrap().clock = clock;
    S_OK
}

unsafe extern "system" fn get_sync_source(me: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: one of ours; the caller gets its own reference.
    let clock = unsafe { this(me) }
        .shared
        .inner
        .lock()
        .unwrap()
        .clock
        .clone();
    unsafe { out.write(clock.map_or(std::ptr::null_mut(), |c| c.into_raw())) };
    S_OK
}

unsafe extern "system" fn enum_pins(me: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    guard_hr(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: one of ours; the caller gets the enumerator's reference.
        let pin: IPin = unsafe { this(me) }.pin.to_interface();
        unsafe { out.write(PinEnum::create(pin).into_raw()) };
        S_OK
    })
}

unsafe extern "system" fn find_pin(me: *mut c_void, id: PCWSTR, out: *mut *mut c_void) -> HRESULT {
    guard_hr(|| {
        if out.is_null() || id.is_null() {
            return E_POINTER;
        }
        // SAFETY: the caller's string and pointer.
        unsafe {
            if id.to_string().ok().as_deref() == Some(PIN_NAME) {
                let pin: IPin = this(me).pin.to_interface();
                out.write(pin.into_raw());
                S_OK
            } else {
                out.write(std::ptr::null_mut());
                VFW_E_NOT_FOUND
            }
        }
    })
}

unsafe extern "system" fn query_filter_info(me: *mut c_void, out: *mut FILTER_INFO) -> HRESULT {
    guard_hr(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: one of ours; the caller releases pGraph.
        let filter = unsafe { this(me) };
        let mut info = FILTER_INFO {
            achName: [0; 128],
            pGraph: std::mem::ManuallyDrop::new(filter.shared.graph()),
        };
        copy_name(&filter.shared.inner.lock().unwrap().name, &mut info.achName);
        unsafe { out.write(info) };
        S_OK
    })
}

unsafe extern "system" fn join_filter_graph(
    me: *mut c_void,
    graph: *mut c_void,
    name: PCWSTR,
) -> HRESULT {
    guard_hr(|| {
        // SAFETY: one of ours; the graph is kept without a reference (DirectShow's rule).
        let filter = unsafe { this(me) };
        let mut inner = filter.shared.inner.lock().unwrap();
        inner.graph = graph;
        if !name.is_null() {
            // SAFETY: the caller's string.
            inner.name = unsafe { name.as_wide() }
                .iter()
                .copied()
                .chain(Some(0))
                .collect();
        }
        S_OK
    })
}

unsafe extern "system" fn query_vendor_info(_: *mut c_void, _: *mut PWSTR) -> HRESULT {
    E_NOTIMPL
}
