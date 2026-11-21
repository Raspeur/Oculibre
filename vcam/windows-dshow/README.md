# Windows — DirectShow virtual camera

Moved into the Rust workspace: [`desktop/vcam-dshow`](../../desktop/vcam-dshow) (the COM DLL,
written in Rust with Microsoft's `windows` crate) and
[`desktop/vcam-shm`](../../desktop/vcam-shm) (the shared memory Oculibre writes the picture into).
See [`docs/research/windows.typ`](../../docs/research/windows.typ).

Visible to DirectShow apps (OBS, Zoom, Discord, Chrome…), on Windows 10 and 11; not to the Windows
Camera app — see `../windows-mf`.
