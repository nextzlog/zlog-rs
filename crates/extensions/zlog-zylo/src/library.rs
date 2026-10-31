use std::ffi::c_void;
use std::path::Path;

pub const EXTENSION: &str = if cfg!(windows) {
    "dll"
} else if cfg!(target_os = "macos") {
    "dylib"
} else {
    "so"
};

pub fn is_plugin_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

pub fn is_any_plugin_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["dll", "dylib", "so"]
            .iter()
            .any(|x| x.eq_ignore_ascii_case(e))
    })
}

pub struct Library {
    handle: *mut c_void,
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    pub fn open(path: &Path) -> Result<Library, String> {
        let handle = sys::open(path)?;
        Ok(Library { handle })
    }

    pub fn symbol(&self, name: &str) -> Option<*const c_void> {
        let p = sys::symbol(self.handle, name);
        (!p.is_null()).then_some(p)
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated wide string; loading runs the library's initialisers,
        // which is the point.
        let h = unsafe { LoadLibraryW(wide.as_ptr()) };
        if h.is_null() {
            let code = std::io::Error::last_os_error();
            return Err(format!("{}: {code}", path.display()));
        }
        Ok(h)
    }

    pub fn symbol(handle: *mut c_void, name: &str) -> *const c_void {
        let Ok(c) = std::ffi::CString::new(name) else {
            return std::ptr::null();
        };
        // SAFETY: a handle from LoadLibraryW and a NUL-terminated name.
        unsafe { GetProcAddress(handle as _, c.as_ptr() as *const u8) }
            .map_or(std::ptr::null(), |f| f as *const c_void)
    }
}

#[cfg(unix)]
mod sys {
    use std::ffi::{c_void, CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| format!("{}: NUL in path", path.display()))?;
        // SAFETY: a NUL-terminated path; loading runs the library's initialisers.
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if h.is_null() {
            // SAFETY: dlerror returns a static NUL-terminated string or null.
            let e = unsafe { libc::dlerror() };
            let why = if e.is_null() {
                "cannot load".to_string()
            } else {
                unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
            };
            return Err(why);
        }
        Ok(h)
    }

    pub fn symbol(handle: *mut c_void, name: &str) -> *const c_void {
        let Ok(c) = CString::new(name) else {
            return std::ptr::null();
        };
        // SAFETY: a handle from dlopen and a NUL-terminated name.
        unsafe { libc::dlsym(handle, c.as_ptr()) as *const c_void }
    }
}

#[cfg(not(any(windows, unix)))]
mod sys {
    use std::ffi::c_void;
    use std::path::Path;

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        Err(format!(
            "{}: plugins are not supported here",
            path.display()
        ))
    }

    pub fn symbol(_: *mut c_void, _: &str) -> *const c_void {
        std::ptr::null()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_is_an_error() {
        let e = Library::open(Path::new("/no/such/plugin.dll"))
            .err()
            .unwrap();
        assert!(!e.is_empty());
    }

    #[test]
    fn plugin_files_are_matched_by_platform() {
        assert!(is_plugin_file(Path::new(&format!(
            "a.{}",
            EXTENSION.to_uppercase()
        ))));
        assert!(!is_plugin_file(Path::new("a.txt")));
        assert!(is_any_plugin_file(Path::new("a.DLL")));
        assert!(is_any_plugin_file(Path::new("a.so")));
        assert!(!is_any_plugin_file(Path::new("a.cfg")));
    }
}
