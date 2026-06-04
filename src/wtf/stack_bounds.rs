//! C++ WTF `StackBounds` current-thread stack bounds.
//!
//! This is a narrow Darwin/aarch64 backend for the dormant JSC
//! `MachineStackMarker` skeleton. Other targets deliberately report unsupported
//! capture instead of manufacturing stack bounds.

#![allow(dead_code)]
#![allow(unsafe_code)]

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WtfStackBounds {
    origin: usize,
    bound: usize,
}

impl WtfStackBounds {
    pub(crate) fn current_thread_stack_bounds() -> Result<Self, WtfStackBoundsError> {
        platform::current_thread_stack_bounds()
    }

    pub(crate) const fn origin_address(self) -> usize {
        self.origin
    }

    pub(crate) const fn bound_address(self) -> usize {
        self.bound
    }

    pub(crate) const fn size(self) -> usize {
        self.origin - self.bound
    }

    pub(crate) const fn contains_address(self, address: usize) -> bool {
        self.origin >= address && address > self.bound
    }

    fn from_origin_and_size(origin: usize, size: usize) -> Result<Self, WtfStackBoundsError> {
        if origin == 0 || size == 0 {
            return Err(WtfStackBoundsError::InvalidStackBounds {
                origin,
                bound: origin,
            });
        }

        let bound = origin
            .checked_sub(size)
            .ok_or(WtfStackBoundsError::StackSizeUnderflow { origin, size })?;
        Self::new(origin, bound)
    }

    const fn new(origin: usize, bound: usize) -> Result<Self, WtfStackBoundsError> {
        if origin <= bound {
            Err(WtfStackBoundsError::InvalidStackBounds { origin, bound })
        } else {
            Ok(Self { origin, bound })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WtfStackBoundsError {
    UnsupportedCurrentThreadCapture {
        target_os: &'static str,
        target_arch: &'static str,
    },
    GetResourceLimitFailed,
    InvalidStackBounds {
        origin: usize,
        bound: usize,
    },
    StackSizeUnderflow {
        origin: usize,
        size: usize,
    },
}

impl fmt::Display for WtfStackBoundsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCurrentThreadCapture {
                target_os,
                target_arch,
            } => write!(
                formatter,
                "current-thread stack capture is unsupported for {target_os}/{target_arch}"
            ),
            Self::GetResourceLimitFailed => {
                write!(formatter, "getrlimit(RLIMIT_STACK) failed")
            }
            Self::InvalidStackBounds { origin, bound } => write!(
                formatter,
                "invalid stack bounds: origin={origin:#x}, bound={bound:#x}"
            ),
            Self::StackSizeUnderflow { origin, size } => write!(
                formatter,
                "stack size underflows origin: origin={origin:#x}, size={size:#x}"
            ),
        }
    }
}

impl std::error::Error for WtfStackBoundsError {}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod platform {
    use std::ffi::{c_int, c_void};
    use std::mem::MaybeUninit;

    use super::{WtfStackBounds, WtfStackBoundsError};

    type PthreadT = *mut c_void;
    type RlimT = u64;

    const RLIMIT_STACK: c_int = 3;
    const RLIM_INFINITY: RlimT = 0x7fff_ffff_ffff_ffff;
    const FALLBACK_MAIN_THREAD_STACK_SIZE: usize = 8 * 1024 * 1024;

    #[repr(C)]
    struct RLimit {
        rlim_cur: RlimT,
        rlim_max: RlimT,
    }

    unsafe extern "C" {
        fn pthread_self() -> PthreadT;
        fn pthread_get_stackaddr_np(thread: PthreadT) -> *mut c_void;
        fn pthread_get_stacksize_np(thread: PthreadT) -> usize;
        fn pthread_main_np() -> c_int;
        fn getrlimit(resource: c_int, rlp: *mut RLimit) -> c_int;
    }

    pub(super) fn current_thread_stack_bounds() -> Result<WtfStackBounds, WtfStackBoundsError> {
        // C++ `StackBounds::currentThreadStackBoundsInternal` uses
        // `getrlimit(RLIMIT_STACK)` for Darwin's main thread because
        // `pthread_get_stacksize_np` can lie there; non-main threads use pthread
        // stack size directly.
        if pthread_main() {
            return current_main_thread_stack_bounds();
        }
        current_non_main_thread_stack_bounds()
    }

    fn current_main_thread_stack_bounds() -> Result<WtfStackBounds, WtfStackBoundsError> {
        let thread = pthread_self_checked();
        let origin = pthread_stack_origin(thread);
        let size = current_main_thread_stack_size()?;
        WtfStackBounds::from_origin_and_size(origin, size)
    }

    fn current_non_main_thread_stack_bounds() -> Result<WtfStackBounds, WtfStackBoundsError> {
        let thread = pthread_self_checked();
        let origin = pthread_stack_origin(thread);
        let size = pthread_stack_size(thread);
        WtfStackBounds::from_origin_and_size(origin, size)
    }

    fn pthread_main() -> bool {
        // SAFETY: `pthread_main_np` takes no arguments and returns whether the
        // current thread is Darwin's process main thread.
        unsafe { pthread_main_np() != 0 }
    }

    fn pthread_self_checked() -> PthreadT {
        // SAFETY: `pthread_self` takes no arguments and returns the current
        // pthread handle, which is used only with pthread stack-bound APIs.
        unsafe { pthread_self() }
    }

    fn pthread_stack_origin(thread: PthreadT) -> usize {
        // SAFETY: `thread` is the current pthread handle returned by
        // `pthread_self`; Darwin documents this non-portable query for pthreads.
        unsafe { pthread_get_stackaddr_np(thread) as usize }
    }

    fn pthread_stack_size(thread: PthreadT) -> usize {
        // SAFETY: `thread` is the current pthread handle returned by
        // `pthread_self`; Darwin documents this non-portable query for pthreads.
        unsafe { pthread_get_stacksize_np(thread) }
    }

    fn current_main_thread_stack_size() -> Result<usize, WtfStackBoundsError> {
        let mut limit = MaybeUninit::<RLimit>::uninit();
        // SAFETY: `limit` points to valid writable storage for the kernel to
        // initialize with the process stack resource limits.
        let result = unsafe { getrlimit(RLIMIT_STACK, limit.as_mut_ptr()) };
        if result != 0 {
            return Err(WtfStackBoundsError::GetResourceLimitFailed);
        }

        // SAFETY: `getrlimit` returned success, so `limit` has been initialized.
        let limit = unsafe { limit.assume_init() };
        if limit.rlim_cur == RLIM_INFINITY {
            return Ok(FALLBACK_MAIN_THREAD_STACK_SIZE);
        }
        usize::try_from(limit.rlim_cur).map_err(|_| WtfStackBoundsError::StackSizeUnderflow {
            origin: usize::MAX,
            size: usize::MAX,
        })
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod platform {
    use super::{WtfStackBounds, WtfStackBoundsError};

    pub(super) fn current_thread_stack_bounds() -> Result<WtfStackBounds, WtfStackBoundsError> {
        Err(WtfStackBoundsError::UnsupportedCurrentThreadCapture {
            target_os: std::env::consts::OS,
            target_arch: std::env::consts::ARCH,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn current_thread_stack_bounds_contains_stack_local() {
        let bounds = WtfStackBounds::current_thread_stack_bounds().expect("stack bounds");
        let local = 0usize;
        let local_address = &local as *const usize as usize;

        assert!(bounds.contains_address(local_address));
        assert!(bounds.origin_address() > bounds.bound_address());
        assert!(bounds.size() > 0);
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    #[test]
    fn current_thread_stack_bounds_reports_unsupported() {
        assert_eq!(
            WtfStackBounds::current_thread_stack_bounds(),
            Err(WtfStackBoundsError::UnsupportedCurrentThreadCapture {
                target_os: std::env::consts::OS,
                target_arch: std::env::consts::ARCH,
            })
        );
    }
}
