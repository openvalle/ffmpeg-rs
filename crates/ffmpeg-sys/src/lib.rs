#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::approx_constant)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::redundant_static_lifetimes)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::ptr_offset_with_cast)]
#![allow(clippy::useless_transmute)]
#![allow(unpredictable_function_pointer_comparisons)]
#![allow(unnecessary_transmutes)]

pub mod runtime;

/// Raw bindings for FFmpeg 7. Only use with a runtime of the same version.
#[allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]
pub mod abi7 {
    include!(concat!(env!("OUT_DIR"), "/abi7/bindings.rs"));
    #[macro_use]
    mod avutil {
        include!(concat!(env!("OUT_DIR"), "/abi7/avutil/mod.rs"));
    }
    pub use crate::runtime;
    pub use avutil::*;
}

/// Raw bindings for FFmpeg 8. Only use with a runtime of the same version.
#[allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]
pub mod abi8 {
    include!(concat!(env!("OUT_DIR"), "/abi8/bindings.rs"));
    #[macro_use]
    mod avutil {
        include!(concat!(env!("OUT_DIR"), "/abi8/avutil/mod.rs"));
    }
    pub use crate::runtime;
    pub use avutil::*;
}

/// Raw bindings for FFmpeg 9. Only use with a runtime of the same version.
#[allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]
pub mod abi9 {
    include!(concat!(env!("OUT_DIR"), "/abi9/bindings.rs"));
    #[macro_use]
    mod avutil {
        include!(concat!(env!("OUT_DIR"), "/abi9/avutil/mod.rs"));
    }
    pub use crate::runtime;
    pub use avutil::*;
}
