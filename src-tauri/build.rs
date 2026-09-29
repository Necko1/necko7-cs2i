fn main() {
    println!("cargo:rerun-if-env-changed=CS2_DASHBOARD_URL");
    tauri_build::build()
}
