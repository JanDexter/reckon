// Some libgit2-sys versions skip emitting `cargo:rustc-link-lib=advapi32`
// on MSVC, which leaves `OpenProcessToken`, `GetNamedSecurityInfoW`,
// `Reg*`, and `Crypt*` unresolved at link time. Always force-link the
// Windows system libs that libgit2 actually uses; they're harmless on
// other targets because this script only runs on Windows.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for lib in ["advapi32", "bcrypt", "ncrypt", "userenv", "ws2_32"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
}
