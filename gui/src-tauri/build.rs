// App commands get an ACL manifest, so each window may only call what its capability grants:
// `main` gets all of them, the game overlay none (ADR 0009). Keep in step with
// `commands::add_handlers`.
const COMMANDS: &[&str] = &[
    "get_games",
    "get_game",
    "get_running_games",
    "save_game",
    "delete_game",
    "untrack_game",
    "create_slot",
    "switch_slot",
    "rename_slot",
    "delete_slot",
    "restore_version",
    "tag_version",
    "set_game_icon",
    "set_game_accent",
    "scan_installed",
    "get_scan_roots",
    "add_scan_root",
    "remove_scan_root",
    "get_monitoring_settings",
    "update_monitoring_settings",
    "screenshots_supported",
    "get_screenshot_path",
    "get_device",
    "update_device",
    "get_sync_settings",
    "update_sync_settings",
    "refresh_sync",
    "adopt_remote_game",
    "apply_remote_version",
    "remove_sync_device",
    "get_retired_versions",
    "get_pruning_settings",
    "update_pruning_settings",
    "preview_pruning",
    "run_pruning",
    "inspect_history_bundle",
    "export_game_history",
    "import_game_history",
    "get_startup_notice",
    "get_recovered_restores",
    "get_prefs_path",
    "pick_folder",
    "pick_file",
    "pick_save_path",
    "confirm",
    "set_tray_labels",
    "set_overlay_config",
    "check_shortcut",
    "name_it_closed",
    "e2e_tray_labels",
];

fn main() {
    println!("cargo:rerun-if-changed=migrations");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");

    // Tauri embeds its Common Controls v6 manifest in the app binary only; the IPC tests build an
    // app too and need it to load.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let manifest = out.join("tests.manifest");
        std::fs::write(&manifest, COMMON_CONTROLS).unwrap();
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}

const COMMON_CONTROLS: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;
