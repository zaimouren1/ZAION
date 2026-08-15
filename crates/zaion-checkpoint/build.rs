// On Windows, libgit2-sys needs advapi32 for ACL/token/registry APIs but doesn't
// always emit the link directive. We do it ourselves so the test binary links correctly.
fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=advapi32");
    }

    // Let the build script know what to re-run on.
    println!("cargo:rerun-if-changed=build.rs");
}
