//! Link-proof for the `phidgets` feature — NO hardware required.
//!
//! This test exists to prove two things about our source-built, statically
//! linked libphidget22 (staged by scripts/build-libphidget22.sh):
//!   1. symbol resolution — the binary linked at all, through `phidget` /
//!      `phidget-sys` / the staged static Phidget22.framework (macOS) or
//!      libphidget22.a (Linux);
//!   2. runtime init — the library's thread/USB machinery actually starts
//!      and stops cleanly in-process.
//!
//! Run with: ./scripts/build-libphidget22.sh && cargo test --features phidgets
//!
//! The whole file compiles to nothing without the feature, so plain
//! `cargo test` never needs the C library.
#![cfg(feature = "phidgets")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn libphidget22_links_and_initializes() {
    // Symbol resolution + a real call into the static lib.
    let version = phidget::library_version().expect("Phidget_getLibraryVersion failed");
    assert!(
        version.contains("Phidget22"),
        "unexpected library version string: {version}"
    );
    let number = phidget::library_version_number().expect("Phidget_getLibraryVersionNumber failed");
    assert!(!number.is_empty(), "library version number came back empty");
    println!("linked libphidget22: {version} (number: {number})");

    // Runtime init: open a manager (spins up the library's enumeration
    // machinery over libusb), give it 100 ms, and shut it down. With no
    // Phidgets hardware attached the attach count stays 0, but callback
    // registration + open + close exercise the full event plumbing.
    let attached = Arc::new(AtomicUsize::new(0));
    let mut manager = phidget::PhidgetManager::new();
    {
        let attached = Arc::clone(&attached);
        manager
            .set_on_attach_handler(move |_phidget| {
                attached.fetch_add(1, Ordering::SeqCst);
            })
            .expect("PhidgetManager_setOnAttachHandler failed");
    }
    manager.open().expect("PhidgetManager_open failed");
    thread::sleep(Duration::from_millis(100));
    manager.close().expect("PhidgetManager_close failed");

    println!(
        "manager open/close OK; devices seen during 100 ms window: {}",
        attached.load(Ordering::SeqCst)
    );
}
