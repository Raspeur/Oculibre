//! The installer's tool for the Media Foundation camera (Windows 11, #17):
//! `oculibre-camera register` makes the "Oculibre" camera known to Windows for every user and for
//! good (it stays after reboots and shows the waiting picture while Oculibre is closed);
//! `oculibre-camera remove` takes it away (uninstall). As administrator.
//!
//! Exit codes: 0 done, 2 not supported (Windows 10: no `MFCreateVirtualCamera`), 1 error.

#[cfg(windows)]
fn main() {
    let command = std::env::args().nth(1).unwrap_or_default();
    let remove = match command.as_str() {
        "register" => false,
        "remove" => true,
        _ => {
            eprintln!("usage: oculibre-camera register|remove");
            std::process::exit(1);
        }
    };
    match oculibre_vcam_mf::system_camera(remove) {
        Ok(()) => println!(
            "{}",
            if remove {
                "camera removed"
            } else {
                "camera registered"
            }
        ),
        Err(oculibre_vcam_mf::CameraError::NotSupported) => {
            println!("no Media Foundation virtual cameras on this Windows (Windows 11 needed)");
            std::process::exit(2);
        }
        Err(oculibre_vcam_mf::CameraError::Failed(e)) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
}
