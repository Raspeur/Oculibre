//! `IMFAttributes` handed on to a store made by Media Foundation (`MFCreateAttributes`): the
//! activator, the source and the stream are attribute stores themselves, as the Frame Server
//! expects. The `windows` crate has no delegation: the 30 methods call the store's own vtable.

/// Implements `IMFAttributes_Impl` for `$ty` (the `_Impl` struct) over its field `$field`
/// (`IMFAttributes`).
macro_rules! delegate_attributes {
    ($ty:ty, $field:ident) => {
        #[allow(non_snake_case)]
        impl windows::Win32::Media::MediaFoundation::IMFAttributes_Impl for $ty {
            fn GetItem(
                &self,
                k: *const windows_core::GUID,
                v: *mut windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
            ) -> windows_core::Result<()> {
                let a = &self.$field;
                unsafe {
                    (windows_core::Interface::vtable(a).GetItem)(
                        windows_core::Interface::as_raw(a),
                        k,
                        v,
                    )
                    .ok()
                }
            }
            fn GetItemType(
                &self,
                k: *const windows_core::GUID,
            ) -> windows_core::Result<windows::Win32::Media::MediaFoundation::MF_ATTRIBUTE_TYPE>
            {
                unsafe { self.$field.GetItemType(k) }
            }
            fn CompareItem(
                &self,
                k: *const windows_core::GUID,
                v: *const windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
            ) -> windows_core::Result<windows_core::BOOL> {
                unsafe { self.$field.CompareItem(k, v) }
            }
            fn Compare(
                &self,
                theirs: windows_core::Ref<windows::Win32::Media::MediaFoundation::IMFAttributes>,
                how: windows::Win32::Media::MediaFoundation::MF_ATTRIBUTES_MATCH_TYPE,
            ) -> windows_core::Result<windows_core::BOOL> {
                unsafe { self.$field.Compare(theirs.as_ref(), how) }
            }
            fn GetUINT32(&self, k: *const windows_core::GUID) -> windows_core::Result<u32> {
                unsafe { self.$field.GetUINT32(k) }
            }
            fn GetUINT64(&self, k: *const windows_core::GUID) -> windows_core::Result<u64> {
                unsafe { self.$field.GetUINT64(k) }
            }
            fn GetDouble(&self, k: *const windows_core::GUID) -> windows_core::Result<f64> {
                unsafe { self.$field.GetDouble(k) }
            }
            fn GetGUID(
                &self,
                k: *const windows_core::GUID,
            ) -> windows_core::Result<windows_core::GUID> {
                unsafe { self.$field.GetGUID(k) }
            }
            fn GetStringLength(&self, k: *const windows_core::GUID) -> windows_core::Result<u32> {
                unsafe { self.$field.GetStringLength(k) }
            }
            fn GetString(
                &self,
                k: *const windows_core::GUID,
                s: windows_core::PWSTR,
                n: u32,
                len: *mut u32,
            ) -> windows_core::Result<()> {
                let a = &self.$field;
                unsafe {
                    (windows_core::Interface::vtable(a).GetString)(
                        windows_core::Interface::as_raw(a),
                        k,
                        s,
                        n,
                        len,
                    )
                    .ok()
                }
            }
            fn GetAllocatedString(
                &self,
                k: *const windows_core::GUID,
                s: *mut windows_core::PWSTR,
                len: *mut u32,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.GetAllocatedString(k, s, len) }
            }
            fn GetBlobSize(&self, k: *const windows_core::GUID) -> windows_core::Result<u32> {
                unsafe { self.$field.GetBlobSize(k) }
            }
            fn GetBlob(
                &self,
                k: *const windows_core::GUID,
                b: *mut u8,
                n: u32,
                len: *mut u32,
            ) -> windows_core::Result<()> {
                let a = &self.$field;
                unsafe {
                    (windows_core::Interface::vtable(a).GetBlob)(
                        windows_core::Interface::as_raw(a),
                        k,
                        b,
                        n,
                        len,
                    )
                    .ok()
                }
            }
            fn GetAllocatedBlob(
                &self,
                k: *const windows_core::GUID,
                b: *mut *mut u8,
                len: *mut u32,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.GetAllocatedBlob(k, b, len) }
            }
            fn GetUnknown(
                &self,
                k: *const windows_core::GUID,
                iid: *const windows_core::GUID,
                out: *mut *mut core::ffi::c_void,
            ) -> windows_core::Result<()> {
                let a = &self.$field;
                unsafe {
                    (windows_core::Interface::vtable(a).GetUnknown)(
                        windows_core::Interface::as_raw(a),
                        k,
                        iid,
                        out,
                    )
                    .ok()
                }
            }
            fn SetItem(
                &self,
                k: *const windows_core::GUID,
                v: *const windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.SetItem(k, v) }
            }
            fn DeleteItem(&self, k: *const windows_core::GUID) -> windows_core::Result<()> {
                unsafe { self.$field.DeleteItem(k) }
            }
            fn DeleteAllItems(&self) -> windows_core::Result<()> {
                unsafe { self.$field.DeleteAllItems() }
            }
            fn SetUINT32(&self, k: *const windows_core::GUID, v: u32) -> windows_core::Result<()> {
                unsafe { self.$field.SetUINT32(k, v) }
            }
            fn SetUINT64(&self, k: *const windows_core::GUID, v: u64) -> windows_core::Result<()> {
                unsafe { self.$field.SetUINT64(k, v) }
            }
            fn SetDouble(&self, k: *const windows_core::GUID, v: f64) -> windows_core::Result<()> {
                unsafe { self.$field.SetDouble(k, v) }
            }
            fn SetGUID(
                &self,
                k: *const windows_core::GUID,
                v: *const windows_core::GUID,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.SetGUID(k, v) }
            }
            fn SetString(
                &self,
                k: *const windows_core::GUID,
                v: &windows_core::PCWSTR,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.SetString(k, *v) }
            }
            fn SetBlob(
                &self,
                k: *const windows_core::GUID,
                b: *const u8,
                n: u32,
            ) -> windows_core::Result<()> {
                let a = &self.$field;
                unsafe {
                    (windows_core::Interface::vtable(a).SetBlob)(
                        windows_core::Interface::as_raw(a),
                        k,
                        b,
                        n,
                    )
                    .ok()
                }
            }
            fn SetUnknown(
                &self,
                k: *const windows_core::GUID,
                v: windows_core::Ref<windows_core::IUnknown>,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.SetUnknown(k, v.as_ref()) }
            }
            fn LockStore(&self) -> windows_core::Result<()> {
                unsafe { self.$field.LockStore() }
            }
            fn UnlockStore(&self) -> windows_core::Result<()> {
                unsafe { self.$field.UnlockStore() }
            }
            fn GetCount(&self) -> windows_core::Result<u32> {
                unsafe { self.$field.GetCount() }
            }
            fn GetItemByIndex(
                &self,
                i: u32,
                k: *mut windows_core::GUID,
                v: *mut windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.GetItemByIndex(i, k, Some(v)) }
            }
            fn CopyAllItems(
                &self,
                dest: windows_core::Ref<windows::Win32::Media::MediaFoundation::IMFAttributes>,
            ) -> windows_core::Result<()> {
                unsafe { self.$field.CopyAllItems(dest.as_ref()) }
            }
        }
    };
}

pub(crate) use delegate_attributes;
