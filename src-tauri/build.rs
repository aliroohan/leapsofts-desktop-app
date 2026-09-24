fn main() {
    // The packaged .app does not run from this repo, so dotenv cannot see ../.env.
    // Bake the desktop env into the binary so `tauri build` follows the file.
    load_desktop_env();

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

fn load_desktop_env() {
    println!("cargo:rerun-if-changed=../.env");
    let Ok(text) = std::fs::read_to_string("../.env") else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key != "VITE_API_URL" && key != "API_BASE" && key != "ERP_URL" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() || value.contains('\n') {
            continue;
        }
        println!("cargo:rustc-env={key}={value}");
    }
}
