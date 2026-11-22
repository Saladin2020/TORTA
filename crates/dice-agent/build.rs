// Embed Windows application icon into dice-agent.exe
fn main() {
    #[cfg(target_os = "windows")]
    {
        let icon = std::path::Path::new("assets/icon.ico");
        if icon.exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            if let Err(e) = res.compile() {
                println!("cargo:warning=winresource icon embed failed: {e}");
            }
        } else {
            println!("cargo:warning=assets/icon.ico missing — exe will use default icon");
        }
    }
}
