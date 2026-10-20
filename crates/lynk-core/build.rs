fn main() {
    // unrar_sys utilise le registre et CryptoAPI sans déclarer advapi32.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
