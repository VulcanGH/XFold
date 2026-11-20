Unicode true
Name "XFold"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\XFold"
InstallDirRegKey HKCU "Software\XFold" "InstallDir"
SetCompressor /SOLID zlib
OutFile "${OUTPUT}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "XFold"
VIAddVersionKey /LANG=1033 "FileDescription" "XFold offline installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "See included notices"

!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_OK|MB_ICONSTOP "XFold requires Windows x64." /SD IDOK
    SetErrorLevel 2
    Quit
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "XFold requires Windows 10 or newer." /SD IDOK
    SetErrorLevel 2
    Quit
  ${EndIf}
FunctionEnd

Section "XFold"
  SetShellVarContext current
  IfFileExists "$INSTDIR\pending\*.*" busy
  IfFileExists "$INSTDIR\previous\*.*" busy
  IfFileExists "$INSTDIR\app\*.*" 0 copy
  IfFileExists "$INSTDIR\app\.xfold-payload" 0 busy
  ClearErrors
  FileOpen $0 "$INSTDIR\app\xfold.exe" a
  IfErrors busy
  FileClose $0

  copy:
    ClearErrors
    SetOutPath "$INSTDIR\pending"
    File /r "${PAYLOAD}\*"
    IfErrors copy_failed
    SetOutPath "$INSTDIR"
    IfFileExists "$INSTDIR\app\*.*" 0 activate
    ClearErrors
    Rename "$INSTDIR\app" "$INSTDIR\previous"
    IfErrors copy_failed
  activate:
    ClearErrors
    Rename "$INSTDIR\pending" "$INSTDIR\app"
    IfErrors restore
    WriteUninstaller "$INSTDIR\uninstall.exe"
    CreateDirectory "$SMPROGRAMS\XFold"
    CreateShortCut "$SMPROGRAMS\XFold\XFold.lnk" "$INSTDIR\app\xfold.exe"
    CreateShortCut "$SMPROGRAMS\XFold\Uninstall XFold.lnk" "$INSTDIR\uninstall.exe"
    WriteRegStr HKCU "Software\XFold" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "DisplayName" "XFold"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "InstallLocation" "$INSTDIR"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "NoRepair" 1
    RMDir /r "$INSTDIR\previous"
    Goto done

  restore:
    IfFileExists "$INSTDIR\previous\*.*" 0 copy_failed
    Rename "$INSTDIR\previous" "$INSTDIR\app"
  copy_failed:
    SetOutPath "$INSTDIR"
    RMDir /r "$INSTDIR\pending"
  busy:
    MessageBox MB_OK|MB_ICONSTOP "XFold could not update this directory. Close XFold and its tools, then retry." /SD IDOK
    SetErrorLevel 2
    Quit
  done:
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  IfFileExists "$INSTDIR\app\.xfold-payload" 0 missing
  IfFileExists "$INSTDIR\removing\*.*" missing
  ClearErrors
  FileOpen $0 "$INSTDIR\app\xfold.exe" a
  IfErrors missing
  FileClose $0
  ClearErrors
  Rename "$INSTDIR\app" "$INSTDIR\removing"
  IfErrors missing
  RMDir /r "$INSTDIR\removing"
  Delete "$SMPROGRAMS\XFold\XFold.lnk"
  Delete "$SMPROGRAMS\XFold\Uninstall XFold.lnk"
  RMDir "$SMPROGRAMS\XFold"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold"
  ReadRegStr $0 HKCU "Software\XFold" "InstallDir"
  StrCmp $0 "$INSTDIR" 0 +2
  DeleteRegValue HKCU "Software\XFold" "InstallDir"
  DeleteRegKey /ifempty HKCU "Software\XFold"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Goto done
  missing:
    MessageBox MB_OK|MB_ICONSTOP "XFold could not be uninstalled. Close XFold and its tools, then retry." /SD IDOK
    SetErrorLevel 2
  done:
SectionEnd
