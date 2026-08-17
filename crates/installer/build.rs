//! Stamps the VP logo into the Windows executable as its resource icon, so Explorer, the
//! taskbar and the shortcut a user makes all show it without the program having to run.
//! Off Windows there is no such resource — the icon is the runtime window icon only, which
//! `theme::window_icon` sets on every platform.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/VP_logo.ico");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icon/VP_logo.ico");
        // Cosmetic, and the resource compiler is the host's: a failure here must not cost a
        // release build, so it is a warning and the binary goes out iconless.
        if let Err(problem) = resource.compile() {
            println!("cargo:warning=could not embed the executable icon: {problem}");
        }
    }
}
