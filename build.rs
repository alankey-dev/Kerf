//! Stamps the git commit into the binary for `kerf --version` and the debug log's session-start line, and
//! on Windows embeds the icon and version information into kerf.exe.

fn main() {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok());
    if let Some(commit) = commit {
        println!("cargo:rustc-env=KERF_GIT_COMMIT={}", commit.trim());
    }
    #[cfg(windows)]
    windows_resources();
}

/// The icon Explorer and the taskbar show, and the details on the file's Properties sheet. The version
/// fields come from Cargo.toml.
#[cfg(windows)]
fn windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("tools/release/icons/kerf.ico").set("ProductName", "Kerf").set("FileDescription", "Kerf").set("OriginalFilename", "kerf.exe");
    if let Err(e) = res.compile() {
        panic!("Couldn't embed the Windows resources: {e}");
    }
}
