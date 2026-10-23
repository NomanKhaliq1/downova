//! `dmhost` (run by the browser) and `dmhost install|uninstall` (run by the installer or by hand).
use dm_host::{serve, Config};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[derive(Deserialize)]
struct Brand {
    product_id: String,
    #[serde(default)]
    extension_ids: Vec<String>,
}

/// Registry keys browsers look in for native messaging hosts (all under HKCU, so no admin rights).
const BROWSERS: [&str; 3] = [
    r"HKCU\Software\Google\Chrome\NativeMessagingHosts",
    r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts",
    r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
];

fn reg(args: &[&str]) -> bool {
    let mut c = Command::new("reg");
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.output().is_ok_and(|o| o.status.success())
}

fn install(brand: &Brand, dir: &std::path::Path) -> Result<(), String> {
    let name = format!("com.{}.host", brand.product_id);
    let manifest = dir.join(format!("{name}.json"));
    let origins: Vec<String> = brand.extension_ids.iter().map(|id| format!("chrome-extension://{id}/")).collect();
    let json = serde_json::json!({
        "name": name,
        "description": format!("{} browser integration", brand.product_id),
        "path": dir.join("dmhost.exe"),
        "type": "stdio",
        "allowed_origins": origins,
    });
    std::fs::write(&manifest, serde_json::to_vec_pretty(&json).unwrap()).map_err(|e| e.to_string())?;
    for b in BROWSERS {
        if !reg(&["add", &format!(r"{b}\{name}"), "/ve", "/t", "REG_SZ", "/d", &manifest.display().to_string(), "/f"]) {
            return Err(format!("could not register in {b}"));
        }
    }
    Ok(())
}

fn uninstall(brand: &Brand, dir: &std::path::Path) {
    let name = format!("com.{}.host", brand.product_id);
    for b in BROWSERS {
        reg(&["delete", &format!(r"{b}\{name}"), "/f"]);
    }
    let _ = std::fs::remove_file(dir.join(format!("{name}.json")));
}

fn main() {
    let brand: Brand = toml::from_str(include_str!("../../brand/brand.toml")).expect("brand/brand.toml");
    let exe = std::env::current_exe().expect("own path");
    let dir = exe.parent().map(PathBuf::from).unwrap_or_default();

    match std::env::args().nth(1).as_deref() {
        Some("install") => match install(&brand, &dir) {
            Ok(()) => println!("registered for Chrome, Edge and Brave"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        Some("uninstall") => uninstall(&brand, &dir),
        _ => {
            let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            let cfg = Config {
                data_dir: appdata.join(&brand.product_id),
                app_path: dir.join("dmd.exe"),
                product_id: brand.product_id,
                start_wait: Duration::from_secs(6),
            };
            if serve(&cfg, std::io::stdin().lock(), std::io::stdout().lock()).is_err() {
                std::process::exit(1);
            }
        }
    }
}
