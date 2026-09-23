fn main() {
    // screencapturekit / Swift concurrency load @rpath/libswift_Concurrency.dylib.
    // Without LC_RPATH the Intel app crashes at launch (DYLD, library missing).
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        println!("cargo:rustc-link-arg=-L/usr/lib/swift");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    }
    tauri_build::build()
}
