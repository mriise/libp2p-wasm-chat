use cargo_toml;

fn main() {
    let toml =
        cargo_toml::Manifest::from_path("Cargo.toml").expect("how do you not have a Cargo.toml???");
    let rust_libp2p_ver = toml
        .dependencies
        .get("libp2p")
        .expect("libp2p-wasm is a hard dependancy of libp2p, do not remove")
        .req();

    println!("cargo:rustc-env=LIBP2P_VERSION={}", rust_libp2p_ver);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
