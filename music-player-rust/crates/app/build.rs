fn main() {
    println!("cargo:rerun-if-changed=assets/MusicPlayer.ico");
    println!("cargo:rerun-if-changed=assets/MusicPlayer-dark.png");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/MusicPlayer.ico");
        let _ = res.compile();
    }
    slint_build::compile("ui/appwindow.slint").expect("Slint build failed");
}
