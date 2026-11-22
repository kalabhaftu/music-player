fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/MusicPlayer.ico");
        let _ = res.compile();
    }
    slint_build::compile("ui/appwindow.slint").expect("Slint build failed");
}
