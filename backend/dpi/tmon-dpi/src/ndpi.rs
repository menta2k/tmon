//! Safe wrapper over libndpi (nDPI 6.0). The only module with `unsafe`
//! calls into nDPI; every block states why it is sound.

use std::ffi::{CStr, c_char};
use std::ptr::NonNull;

use ndpi_sys as sys;
use tmon_dpi_proto::{CatalogEntry, MAX_CATALOG, MAX_NAME, MAX_STR};

use crate::classify::GENERIC_CARRIERS;
use crate::engine::{Engine, NdpiConfidence, NdpiResult};

/// Copy a C string nDPI owns (static protocol tables) into Rust.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated string valid for this call.
unsafe fn owned(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: non-null and NUL-terminated per the caller's contract.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// One nDPI detection module (`ndpi_init_detection_module` +
/// `ndpi_finalize_initialization`); exits the module on drop.
pub struct Module {
    m: NonNull<sys::ndpi_detection_module_struct>,
}

impl Module {
    /// Initialize nDPI. Install the counting allocator (`alloc::install`)
    /// before calling this.
    pub fn new() -> Result<Self, String> {
        // SAFETY: a NULL global context is allowed (ndpi_api.h); the
        // license value is a valid `ndpi_license_type`.
        let raw = unsafe {
            sys::ndpi_init_detection_module(
                std::ptr::null_mut(),
                sys::NDPI_LICENSE_NOT_FOR_PROFIT_LGPL,
            )
        };
        let m = NonNull::new(raw).ok_or("ndpi_init_detection_module failed")?;
        // SAFETY: `m` was just returned by ndpi_init_detection_module.
        let rc = unsafe { sys::ndpi_finalize_initialization(m.as_ptr()) };
        let module = Self { m };
        if rc != 0 {
            return Err(format!("ndpi_finalize_initialization failed ({rc})"));
        }
        Ok(module)
    }

    /// "nDPI <revision> (API <n>)", at most `MAX_STR` bytes.
    pub fn version() -> String {
        // SAFETY: ndpi_revision returns a static NUL-terminated string.
        let rev = unsafe { owned(sys::ndpi_revision()) };
        // SAFETY: no arguments, returns a plain integer.
        let api = unsafe { sys::ndpi_get_api_version() };
        let mut v = format!("nDPI {rev} (API {api})");
        v.truncate(MAX_STR);
        v
    }

    pub fn proto_name(&self, id: u16) -> String {
        // SAFETY: the module is initialized; nDPI maps invalid ids to
        // "Unknown" and returns a pointer into its static tables.
        unsafe { owned(sys::ndpi_get_proto_name(self.m.as_ptr(), id)) }
    }

    pub fn category_name(&self, cat: sys::ndpi_protocol_category_t) -> String {
        // SAFETY: as above; returns a static (or module-owned) string.
        unsafe { owned(sys::ndpi_category_get_name(self.m.as_ptr(), cat)) }
    }

    fn category_of(&self, id: u16) -> sys::ndpi_protocol_category_t {
        // SAFETY: `ndpi_protocol` is plain data (integers and one unused
        // pointer); all-zero is a valid "unknown" value.
        let mut p: sys::ndpi_protocol = unsafe { std::mem::zeroed() };
        p.proto.app_protocol = id;
        // SAFETY: initialized module and a valid protocol value.
        unsafe { sys::ndpi_get_proto_category(self.m.as_ptr(), p) }
    }

    /// Reportable applications and their categories (Hello catalog).
    pub fn catalog(&self) -> Vec<CatalogEntry> {
        // SAFETY: the module is initialized (finalized), as required.
        let n = unsafe { sys::ndpi_get_num_protocols(self.m.as_ptr()) };
        let n = u16::try_from(n).unwrap_or(u16::MAX);
        let ok = |s: &str| !s.is_empty() && s.len() <= MAX_NAME && s.is_ascii();
        let mut out: Vec<CatalogEntry> = (1..n)
            .map(|id| {
                (
                    self.proto_name(id),
                    self.category_name(self.category_of(id)),
                )
            })
            .filter(|(app, cat)| ok(app) && (cat.is_empty() || ok(cat)))
            .filter(|(app, _)| !GENERIC_CARRIERS.iter().any(|g| g.eq_ignore_ascii_case(app)))
            .map(|(app, category)| CatalogEntry { app, category })
            .collect();
        out.sort_by(|a, b| a.app.cmp(&b.app));
        out.dedup_by(|a, b| a.app.eq_ignore_ascii_case(&b.app));
        out.truncate(MAX_CATALOG);
        out
    }

    fn result(&self, flow: &Flow, p: &sys::ndpi_protocol) -> NdpiResult {
        // SAFETY: `flow.f` is a live flow; the shim reads the field with the
        // C compiler's layout.
        let confidence = unsafe { sys::tmon_ndpi_flow_confidence(flow.f.as_ptr()) };
        // SAFETY: the shim returns the flow's fixed host buffer and its size;
        // the read is bounded by that size and the flow outlives this call.
        let host = unsafe {
            let p = sys::tmon_ndpi_flow_host(flow.f.as_ptr()).cast::<u8>();
            let len = sys::tmon_ndpi_flow_host_len() as usize;
            CStr::from_bytes_until_nul(std::slice::from_raw_parts(p, len))
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        NdpiResult {
            app: self.proto_name(p.proto.app_protocol),
            master: self.proto_name(p.proto.master_protocol),
            category: self.category_name(p.category),
            confidence: Some(NdpiConfidence::from_raw(confidence.max(0) as u32)),
            host,
        }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // SAFETY: `m` came from ndpi_init_detection_module and is dropped once;
        // all flows borrow the module mutably only during calls.
        unsafe { sys::ndpi_exit_detection_module(self.m.as_ptr()) };
    }
}

/// One `ndpi_flow_struct`, zeroed on allocation, freed with `ndpi_free_flow`.
pub struct Flow {
    f: NonNull<sys::ndpi_flow_struct>,
}

impl Drop for Flow {
    fn drop(&mut self) {
        // SAFETY: `f` came from ndpi_flow_malloc and is freed exactly once;
        // ndpi_free_flow releases its buffers and the struct itself.
        unsafe { sys::ndpi_free_flow(self.f.as_ptr()) };
    }
}

impl Engine for Module {
    type Flow = Flow;

    fn new_flow(&mut self) -> Option<Flow> {
        // SAFETY: plain size query.
        let size = unsafe { sys::ndpi_detection_get_sizeof_ndpi_flow_struct() } as usize;
        // SAFETY: allocation through nDPI's (counting) flow allocator.
        let raw = unsafe { sys::ndpi_flow_malloc(size) }.cast::<sys::ndpi_flow_struct>();
        let f = NonNull::new(raw)?;
        // SAFETY: `raw` points to `size` writable bytes; nDPI requires a
        // zeroed flow before the first packet.
        unsafe { std::ptr::write_bytes(raw.cast::<u8>(), 0, size) };
        Some(Flow { f })
    }

    fn process(&mut self, flow: &mut Flow, l3: &[u8], ts_ms: u64) -> NdpiResult {
        let len = u16::try_from(l3.len()).unwrap_or(u16::MAX);
        // SAFETY: the module and flow are live; `l3` is valid for `len`
        // bytes (the codec caps it at MAX_L3 < u16::MAX); input_info may be NULL.
        let p = unsafe {
            sys::ndpi_detection_process_packet(
                self.m.as_ptr(),
                flow.f.as_ptr(),
                l3.as_ptr(),
                len,
                ts_ms,
                std::ptr::null_mut(),
            )
        };
        self.result(flow, &p)
    }

    fn giveup(&mut self, flow: &mut Flow) -> NdpiResult {
        // SAFETY: the module and flow are live.
        let p = unsafe { sys::ndpi_detection_giveup(self.m.as_ptr(), flow.f.as_ptr()) };
        self.result(flow, &p)
    }

    fn mem_bytes(&self) -> u64 {
        crate::alloc::live_bytes() as u64
    }
}
