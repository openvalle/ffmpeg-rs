// Compiled as an isolated executable by dynamic_generator.rs, never part of the library.
mod runtime {
    use std::sync::OnceLock;
    #[derive(Debug)]
    pub struct LoadError(pub String);
    impl std::fmt::Display for LoadError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt(f)
        }
    }
    pub struct Version;
    impl Version {
        pub fn major(&self) -> u32 {
            9
        }
    }
    pub enum Functions {
        V9(crate::abi9::DynamicFunctions),
        Other,
    }
    pub struct Runtime {
        pub functions: Functions,
        pub version: Version,
    }
    pub struct Libraries;
    impl Libraries {
        pub unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, LoadError> {
            unsafe extern "C" fn allocate() -> *mut u8 {
                std::ptr::null_mut()
            }
            if name == b"av_frame_alloc\0" && crate::mode() != "required-missing" {
                let function: unsafe extern "C" fn() -> *mut u8 = allocate;
                // The synthetic declaration above is the only successful lookup in this fixture.
                return Ok(unsafe { std::mem::transmute_copy(&function) });
            }
            Err(LoadError("symbol missing".into()))
        }
    }
    pub fn load() -> Result<&'static Runtime, LoadError> {
        static INSTANCE: OnceLock<Runtime> = OnceLock::new();
        if let Some(runtime) = INSTANCE.get() {
            return Ok(runtime);
        }
        let functions = if crate::mode().starts_with("wrong-") {
            Functions::Other
        } else {
            Functions::V9(unsafe { crate::abi9::DynamicFunctions::load(&Libraries)? })
        };
        Ok(INSTANCE.get_or_init(|| Runtime {
            functions,
            version: Version,
        }))
    }
    pub fn loaded() -> &'static Runtime {
        load().unwrap()
    }
}
fn mode() -> String {
    std::env::args().nth(1).unwrap()
}
fn main() {
    match mode().as_str() {
        "available" => {
            crate::abi9::check().unwrap();
            assert!(crate::abi9::has_symbol("av_frame_alloc").unwrap());
            assert!(!crate::abi9::has_symbol("av_optional_probe").unwrap());
            assert!(!crate::abi9::has_symbol("unknown").unwrap());
            // Preserve compatibility with C callback/function pointer slots.
            let callback: unsafe extern "C" fn() -> *mut u8 = crate::abi9::av_frame_alloc;
            assert!(unsafe { callback() }.is_null());
        }
        "required-missing" => assert!(crate::abi9::check().is_err()),
        "wrong-check" => {
            assert!(
                crate::abi9::check()
                    .err()
                    .unwrap()
                    .0
                    .contains("ABI mismatch")
            );
            assert!(crate::abi9::has_symbol("av_frame_alloc").is_err());
        }
        mode => {
            let _ = std::panic::catch_unwind(|| unsafe {
                if mode == "optional-missing" {
                    crate::abi9::av_optional_probe();
                } else {
                    crate::abi9::av_frame_alloc();
                }
            });
            println!("unwound");
        }
    }
}
