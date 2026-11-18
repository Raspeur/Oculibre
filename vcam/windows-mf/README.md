# Windows 11 — Media Foundation virtual camera

In the Rust workspace: [`desktop/vcam-mf`](../../desktop/vcam-mf). A media source DLL loaded by
Windows' Frame Server service, and `oculibre-camera.exe`, which the installer runs to register the
camera for every user (it stays registered, even while Oculibre is closed). It reads the picture
Oculibre writes into a global shared memory block (`desktop/vcam-shm`).

Windows shows it as "Oculibre (Caméra virtuelle Windows)". Every app sees it, including the
Windows Camera app and DirectShow apps, but Windows gives it to one app at a time unless "Allow
multiple apps to use the camera" is on: for OBS + Discord at once, use the DirectShow camera
("Oculibre", [`desktop/vcam-dshow`](../../desktop/vcam-dshow)). See
[`docs/research/windows.typ`](../../docs/research/windows.typ).
