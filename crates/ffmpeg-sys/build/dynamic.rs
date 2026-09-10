// Valle extension to the upstream bindings: keep signatures generated from the actual headers.
use quote::quote;
use std::{fs, path::Path};
use syn::{ForeignItem, Item};

pub fn generate(output: &Path, major: u32) {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let shared = manifest;
    let required_path = shared.join("required-symbols.txt");
    println!("cargo:rerun-if-changed={}", required_path.display());
    let required_text = fs::read_to_string(required_path).unwrap();
    let required: std::collections::HashSet<_> = required_text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    let original = fs::read_to_string(output.join("bindings.original.rs")).unwrap();
    let mut file = syn::parse_file(&original).unwrap();
    let mut fields = Vec::new();
    let mut names = Vec::new();
    let mut availability = Vec::new();
    let mut loads = Vec::new();
    let mut wrappers = Vec::new();
    for item in &mut file.items {
        let Item::ForeignMod(block) = item else {
            continue;
        };
        block.items.retain(|item| {
            let name = match item {
                ForeignItem::Fn(f) => f.sig.ident.to_string(),
                ForeignItem::Static(s) => s.ident.to_string(),
                _ => return true,
            };
            if !["av", "sws", "swr"].iter().any(|p| name.starts_with(p)) {
                return false;
            }
            // No Valle or ffmpeg-next consumer uses these globals or variadic APIs.
            let ForeignItem::Fn(f) = item else {
                return false;
            };
            if f.sig.variadic.is_some() || name.starts_with("av_vk_")
            {
                return false;
            }
            let id = &f.sig.ident;
            names.push(name.clone());
            availability.push(quote! { #name => functions.#id.is_some() });
            let args = &f.sig.inputs;
            let result = &f.sig.output;
            let values = args.iter().map(|arg| match arg {
                syn::FnArg::Typed(arg) => &arg.pat,
                _ => unreachable!(),
            });
            let symbol = format!("{name}\0");
            let attrs = &f.attrs;
            fields.push(quote! { pub(crate) #id: Option<unsafe extern "C" fn(#args) #result> });
            if required.contains(name.as_str()) {
                loads.push(quote! { #id: Some(unsafe { libraries.symbol(#symbol.as_bytes())? }) });
            } else {
                loads.push(quote! { #id: unsafe { libraries.symbol(#symbol.as_bytes()).ok() } });
            }
            wrappers.push(quote! {
                #(#attrs)*
                #[inline]
                pub unsafe extern "C" fn #id(#args) #result {
                    // All production entry points call runtime::load() and return its errors first.
                    unsafe { (functions().#id.expect(concat!("FFmpeg runtime does not export ", stringify!(#id))))(#(#values),*) }
                }
            });
            false
        });
    }
    file.items
        .retain(|item| !matches!(item, Item::ForeignMod(b) if b.items.is_empty()));
    names.sort();
    fs::write(output.join("symbols.txt"), names.join("\n")).unwrap();
    let variant = quote::format_ident!("V{major}");
    let generated = quote! {
        #file
        #(#wrappers)*
        /// Check the selected runtime before using this ABI's typed wrappers or native pointers.
        pub fn check() -> Result<&'static crate::runtime::Runtime, crate::runtime::LoadError> {
            let runtime = crate::runtime::load()?;
            if matches!(&runtime.functions, crate::runtime::Functions::#variant(_)) {
                Ok(runtime)
            } else {
                Err(crate::runtime::LoadError(format!(
                    "FFmpeg ABI mismatch: attempted ABI {} with loaded FFmpeg {}",
                    #major, runtime.version.major(),
                )))
            }
        }
        /// Whether a generated function is available in this ABI and loaded library set.
        /// Unknown names and functions excluded by Cargo features return false.
        pub fn has_symbol(name: &str) -> Result<bool, crate::runtime::LoadError> {
            check()?;
            let functions = functions();
            Ok(match name { #(#availability,)* _ => false })
        }
        fn functions() -> &'static DynamicFunctions {
            match &crate::runtime::loaded().functions {
                crate::runtime::Functions::#variant(functions) => functions,
                _ => panic!("FFmpeg ABI mismatch: attempted to use an unselected backend"),
            }
        }
        pub(crate) struct DynamicFunctions { #(#fields,)* }
        impl DynamicFunctions {
            pub(crate) unsafe fn load(libraries: &crate::runtime::Libraries) -> Result<Self, crate::runtime::LoadError> {
                Ok(Self { #(#loads,)* })
            }
        }
    };
    let parsed = syn::parse2(generated).expect("generated dynamic bindings");
    fs::write(output.join("bindings.rs"), prettyplease::unparse(&parsed)).unwrap();
}
