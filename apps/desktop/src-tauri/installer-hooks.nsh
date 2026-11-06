; Register only a fixed open-only URI under the current user's registry hive.
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\qqnt-storage-manager" "" "URL:QQNT Storage Manager"
  WriteRegStr HKCU "Software\Classes\qqnt-storage-manager" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\qqnt-storage-manager\shell\open\command" "" '$\"$INSTDIR\qqnt-storage-manager.exe$\" $\"%1$\"'
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  ReadRegStr $0 HKCU "Software\Classes\qqnt-storage-manager\shell\open\command" ""
  StrCmp $0 '$\"$INSTDIR\qqnt-storage-manager.exe$\" $\"%1$\"' 0 +2
  DeleteRegKey HKCU "Software\Classes\qqnt-storage-manager"
!macroend
