fn main() {
    // The `phidgets` feature links libphidget22, which must first be staged
    // by scripts/build-libphidget22.sh. Everything below is inert for the
    // default (feature-off) build. See src/capture/PHIDGETS-SPIKE.md.
    if std::env::var_os("CARGO_FEATURE_PHIDGETS").is_some() {
        link_phidgets();
    }

    tauri_build::build()
}

/// Wire the final link against the staged libphidget22 prefix.
///
/// `phidget-sys`'s own build.rs emits the link-lib directives
/// (`framework=Phidget22` on macOS, `-lphidget22` elsewhere); our job is only
/// to make its search resolve to OUR static, source-built artifacts:
///   - macOS: a staged static `Phidget22.framework` (libphidget22.a merged
///     with libusb-1.0.a) — the only shape the crate will link on macOS —
///     plus the system frameworks the static code needs.
///   - Linux: the staged lib dir (only the .a is present, so `-lphidget22`
///     resolves statically) plus the system libusb.
///   - Windows: untested; `phidget-sys` honors `PHIDGET_ROOT` there. See the
///     spike report for the recommended CI recipe.
fn link_phidgets() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR unset");
    println!("cargo:rerun-if-env-changed=DROPTIME_PHIDGET_PREFIX");
    let prefix = std::env::var("DROPTIME_PHIDGET_PREFIX")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(&manifest_dir)
                .join("target")
                .join("phidget-prefix")
        });

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS unset");
    match target_os.as_str() {
        "macos" => {
            let framework_bin = prefix.join("Frameworks/Phidget22.framework/Phidget22");
            assert!(
                framework_bin.is_file(),
                "phidgets feature: staged Phidget22.framework not found at {} — \
                 run scripts/build-libphidget22.sh first",
                framework_bin.display()
            );
            println!(
                "cargo:rustc-link-search=framework={}",
                prefix.join("Frameworks").display()
            );
            // System deps of the static libphidget22 + libusb code. (libusb's
            // Darwin backend needs IOKit/CoreFoundation, and Security for its
            // entitlement check in darwin_detach_kernel_driver.)
            //
            // Emitted as rustc-link-arg, NOT rustc-link-lib: link-lib rides
            // this lib target's rlib metadata, which a linkable target drops
            // unless it references the lib (the phidgets_link integration
            // test uses only `phidget`, so the metadata never reaches ld).
            // link-arg goes on the command line of every linkable target.
            for framework in ["IOKit", "CoreFoundation", "Security"] {
                println!("cargo:rustc-link-arg=-framework");
                println!("cargo:rustc-link-arg={framework}");
            }
        }
        "linux" => {
            let static_lib = prefix.join("lib/libphidget22.a");
            assert!(
                static_lib.is_file(),
                "phidgets feature: staged libphidget22.a not found at {} — \
                 run scripts/build-libphidget22.sh first",
                static_lib.display()
            );
            println!(
                "cargo:rustc-link-search=native={}",
                prefix.join("lib").display()
            );
            // libphidget22's USB transport; dynamic system libusb is the norm
            // on Linux (libusb-1.0-0-dev at build time). link-arg (see the
            // macOS note) also lands after `-lphidget22` on the command
            // line, which GNU ld's left-to-right archive resolution needs.
            println!("cargo:rustc-link-arg=-lusb-1.0");
        }
        // Windows: phidget-sys resolves phidget22.lib via PHIDGET_ROOT on its
        // own; nothing is staged by our script (yet). Other targets: let the
        // link fail loudly rather than guess.
        _ => {}
    }
}
