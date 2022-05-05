use wasm_bindgen::prelude::*;

#[cfg(feature = "console_error_panic_hook")]
#[wasm_bindgen]
pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function at least once during initialization, and then
    // we will get better error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme

    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

pub fn log_meta() {
    let lib_ver = format_args!("Rust-Libp2p: v{}\n", env!("LIBP2P_VERSION")).to_string();
    let wrapper_ver = format_args!("WASM-Wrapper: v{}", env!("CARGO_PKG_VERSION")).to_string();

    gloo::console::info!("%c Rust-Libp2p-WASM\n", "color: grey", lib_ver, wrapper_ver);
}
