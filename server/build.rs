// Embeds the brand icon and version info into the .exe (name, company and icon come from brand/brand.toml).
fn main() {
    println!("cargo:rerun-if-changed=../brand/brand.toml");
    println!("cargo:rerun-if-changed=../brand/assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let brand = std::fs::read_to_string("../brand/brand.toml").unwrap_or_default();
    let value = |key: &str| {
        brand.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().split('"').nth(1).unwrap_or("").to_string())
        })
    };
    let (name, company) = (value("name").unwrap_or_default(), value("company").unwrap_or_default());
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../brand/assets/app.ico");
    res.set("ProductName", &name).set("FileDescription", &name).set("CompanyName", &company).set("LegalCopyright", &format!("(c) {company}"));
    if let Err(e) = res.compile() {
        // the app works without an icon; do not fail the build on machines without the Windows SDK
        println!("cargo:warning=could not embed the icon: {e}");
    }
}