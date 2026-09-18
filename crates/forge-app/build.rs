fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    #[cfg(windows)]
    winresource::WindowsResource::new().set_icon("assets/icon.ico").compile().unwrap();
}
