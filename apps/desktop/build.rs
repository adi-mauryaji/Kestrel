fn main() {
    // The app's own commands, each allowed only where a capability grants it (capabilities/).
    let commands = tauri_build::AppManifest::new().commands(&["first_start", "use_notes"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("the desktop app's build step failed");
}
