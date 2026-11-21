; Oculibre's Windows installer (#17): the steps Tauri's NSIS installer runs around its own.
;
; - The virtual cameras: the DirectShow DLL, 64-bit (64-bit apps) and 32-bit (old 32-bit apps),
;   and on Windows 11 the Media Foundation source (loaded by the Frame Server service), registered
;   with regsvr32 after the files are copied, unregistered before they are removed. The Media
;   Foundation camera is then made known to Windows for every user by oculibre-camera.exe (it
;   does nothing on Windows 10). An update takes effect in the Frame Server at the next restart.
; - The firewall: phones connect to Oculibre (port 47100, Wi-Fi) and look for it (mDNS).
; - Updating while an app (OBS, Zoom…) has the camera open: Windows does not let a loaded DLL be
;   replaced, but lets it be renamed. The old one is moved aside and deleted at the next restart.
;
; The installer is a 32-bit program: on 64-bit Windows, $SYSDIR is the 32-bit system folder.
; The 64-bit regsvr32 is reached through Sysnative.

!macro OCULIBRE_REGSVR64 ARGS
  IfFileExists "$WINDIR\Sysnative\regsvr32.exe" 0 +3
    ExecWait '"$WINDIR\Sysnative\regsvr32.exe" ${ARGS}'
    Goto +2
    ExecWait '"$SYSDIR\regsvr32.exe" ${ARGS}'
!macroend

!macro OCULIBRE_REGSVR32 ARGS
  IfFileExists "$WINDIR\SysWOW64\regsvr32.exe" 0 +2
    ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" ${ARGS}'
!macroend

; Moves a DLL an app may still hold out of the way (unique name), to delete at the next restart.
!macro OCULIBRE_SET_ASIDE FILE
  IfFileExists "${FILE}" 0 +5
    GetTempFileName $0 "$INSTDIR"
    Delete $0
    Rename "${FILE}" $0
    Delete /REBOOTOK $0
!macroend

; Old DLLs set aside by an earlier update or uninstall, no longer in use: delete them now
; (those still loaded stay, deleted at the next restart).
!macro OCULIBRE_CLEAN_ASIDE
  Delete "$INSTDIR\ns*.tmp"
!macroend

; adb's server keeps running after Oculibre closes, and holds adb.exe and its DLLs: stop it, and
; since it takes a moment to exit, also move its files aside (renaming works even while loaded).
!macro OCULIBRE_STOP_ADB
  IfFileExists "$INSTDIR\adb\adb.exe" 0 +2
    nsExec::Exec '"$INSTDIR\adb\adb.exe" kill-server'
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\adb\adb.exe"
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\adb\AdbWinApi.dll"
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\adb\AdbWinUsbApi.dll"
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro OCULIBRE_CLEAN_ASIDE
  !insertmacro OCULIBRE_STOP_ADB
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\oculibre_vcam_mf.dll"
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\oculibre_vcam_dshow.dll"
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\x86\oculibre_vcam_dshow.dll"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro OCULIBRE_REGSVR64 '/s "$INSTDIR\oculibre_vcam_dshow.dll"'
  !insertmacro OCULIBRE_REGSVR32 '/s "$INSTDIR\x86\oculibre_vcam_dshow.dll"'
  !insertmacro OCULIBRE_REGSVR64 '/s "$INSTDIR\oculibre_vcam_mf.dll"'
  nsExec::Exec '"$INSTDIR\oculibre-camera.exe" register'
  Pop $0
  ; Phones reach Oculibre over the network only when Wi-Fi is on in its settings, encrypted and
  ; paired: the rule is for Oculibre's program only.
  nsExec::Exec 'netsh advfirewall firewall delete rule name="Oculibre"'
  Pop $0
  nsExec::Exec 'netsh advfirewall firewall add rule name="Oculibre" dir=in action=allow program="$INSTDIR\oculibre-app.exe" enable=yes profile=any'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro OCULIBRE_STOP_ADB
  nsExec::Exec '"$INSTDIR\oculibre-camera.exe" remove'
  Pop $0
  !insertmacro OCULIBRE_REGSVR64 '/u /s "$INSTDIR\oculibre_vcam_mf.dll"'
  !insertmacro OCULIBRE_REGSVR64 '/u /s "$INSTDIR\oculibre_vcam_dshow.dll"'
  !insertmacro OCULIBRE_REGSVR32 '/u /s "$INSTDIR\x86\oculibre_vcam_dshow.dll"'
  nsExec::Exec 'netsh advfirewall firewall delete rule name="Oculibre"'
  Pop $0
  ; Still loaded by an app: moved aside, deleted at the next restart.
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\oculibre_vcam_dshow.dll"
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\x86\oculibre_vcam_dshow.dll"
  !insertmacro OCULIBRE_SET_ASIDE "$INSTDIR\oculibre_vcam_mf.dll"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro OCULIBRE_CLEAN_ASIDE
  RMDir "$INSTDIR\x86"
  RMDir "$INSTDIR\adb"
  RMDir /REBOOTOK "$INSTDIR"
!macroend
