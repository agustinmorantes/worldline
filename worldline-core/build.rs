#![allow(clippy::collapsible_if)]

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=UPDATE_MANIFEST");
    println!("cargo:rerun-if-changed=assets/manifest.yaml");

    if env::var("UPDATE_MANIFEST").unwrap_or_default() == "1" {
        match download_manifest() {
            Ok(()) => {}
            Err(e) => println!("cargo:warning=Failed to update Ludusavi manifest: {}", e),
        }
    }
}

fn download_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = Path::new("assets/manifest.yaml");
    let etag_path = Path::new("assets/manifest.etag");

    let mut etag = String::new();
    if etag_path.exists() {
        etag = fs::read_to_string(etag_path)?
            .trim()
            .trim_matches('"')
            .to_string();
    }

    let url =
        "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
    let mut req = ureq::get(url);
    if !etag.is_empty() {
        req = req.set("If-None-Match", &format!("\"{}\"", etag));
    }

    match req.call() {
        Ok(resp) => {
            let new_etag = resp
                .header("ETag")
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
            let body = resp.into_string()?;
            fs::write(manifest_path, body)?;
            fs::write(etag_path, new_etag)?;
            println!("cargo:warning=Ludusavi manifest updated successfully.");
        }
        Err(ureq::Error::Status(304, _)) => {
            println!("cargo:warning=Ludusavi manifest has not changed (304).");
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}
