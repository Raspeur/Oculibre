# Windows 11 — Media Foundation virtual camera

Planned in Rust, next to the DirectShow one ([`desktop/vcam-dshow`](../../desktop/vcam-dshow)):
a media source DLL loaded by the Frame Server service, registered with `MFCreateVirtualCamera`
(Windows 11 only). See [`docs/research/windows.typ`](../../docs/research/windows.typ).

Visible to every app, including the Windows Camera app.
