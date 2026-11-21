//! Registers the Media Foundation camera like Oculibre does, finds it among the system's cameras,
//! and reads pictures through the Frame Server like an app: rate, size, one picture saved as raw
//! NV12. Windows 11, the DLL registered first (regsvr32):
//! `cargo run -p oculibre-vcam-mf --example mf_check -- [frames] [out.nv12]`, or `-- hold <seconds>`
//! to only keep the camera registered while another app tries it.

#[cfg(windows)]
fn main() -> windows_core::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let mut args = std::env::args().skip(1);
    let frames: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let out = args.next();
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let id = format!("{{{:?}}}", oculibre_vcam_mf::CLSID_OCULIBRE_MF);
        // `system`: a camera that stays registered after this program (and reboots); `remove`:
        // remove that one again.
        let mode = std::env::args().nth(1).unwrap_or_default();
        let lifetime = if mode == "system" || mode == "remove" {
            MFVirtualCameraLifetime_System
        } else {
            MFVirtualCameraLifetime_Session
        };
        let camera = MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            lifetime,
            MFVirtualCameraAccess_CurrentUser,
            &HSTRING::from("Oculibre"),
            &HSTRING::from(id.as_str()),
            None,
        )?;
        camera.Start(None)?;
        println!("virtual camera started ({id})");
        if mode == "system" {
            println!("registered for the system: stays after this program");
            return Ok(());
        }
        if mode == "remove" {
            camera.Remove()?;
            println!("system camera removed");
            return Ok(());
        }
        // `hold <seconds>`: only keep the camera registered, for other apps to try it.
        if let Some(seconds) = std::env::args().nth(1).filter(|a| a == "hold").and(std::env::args().nth(2)) {
            std::thread::sleep(std::time::Duration::from_secs(seconds.parse().unwrap_or(30)));
            camera.Remove()?;
            println!("virtual camera removed");
            return Ok(());
        }

        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.unwrap();
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;
        let mut list = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attributes, &mut list, &mut count)?;
        let devices = std::slice::from_raw_parts(list, count as usize);
        let mut ours = None;
        for d in devices.iter().flatten() {
            let mut name = windows_core::PWSTR::null();
            let mut len = 0;
            d.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut len)?;
            let name = name.to_string().unwrap_or_default();
            println!("camera: {name}");
            if name.starts_with("Oculibre") {
                ours = Some(d.clone());
            }
        }
        let Some(device) = ours else {
            println!("Oculibre not found");
            camera.Remove()?;
            return Ok(());
        };
        let source: IMFMediaSource = device.ActivateObject()?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, None)?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let current = reader.GetCurrentMediaType(stream)?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE)?;
        println!(
            "format: {}x{} NV12={}",
            size >> 32,
            size & 0xffff_ffff,
            current.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12
        );
        let started = std::time::Instant::now();
        let mut got = 0;
        let mut saved = false;
        while got < frames {
            let mut flags = 0u32;
            let mut sample = None;
            reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))?;
            if let Some(sample) = sample {
                got += 1;
                if let (Some(path), false) = (&out, saved) {
                    if got > 10 {
                        let buffer = sample.ConvertToContiguousBuffer()?;
                        let mut data = std::ptr::null_mut();
                        let mut len = 0u32;
                        buffer.Lock(&mut data, None, Some(&mut len))?;
                        std::fs::write(path, std::slice::from_raw_parts(data, len as usize))
                            .unwrap();
                        buffer.Unlock()?;
                        saved = true;
                    }
                }
            }
        }
        let secs = started.elapsed().as_secs_f64();
        println!(
            "{got} pictures in {secs:.2} s: {:.1} per second",
            f64::from(got) / secs
        );
        source.Shutdown()?;
        camera.Remove()?;
        println!("virtual camera removed");
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows 11 only");
}
