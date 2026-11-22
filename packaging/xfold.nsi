Unicode true
Name "XFold"
RequestExecutionLevel admin
InstallDir "$PROGRAMFILES64\XFold"
# InstallDirRegKey uses the native registry view of the x64 NSIS stub.
InstallDirRegKey HKLM "Software\XFold" "InstallDir"
SetCompressor /SOLID zlib
OutFile "${OUTPUT}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "XFold"
VIAddVersionKey /LANG=1033 "FileDescription" "XFold Windows installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "See included notices"

!define MUI_ICON "../ui/xfold.ico"
!define MUI_UNICON "../ui/xfold.ico"
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!define MUI_ABORTWARNING
!ifdef MKVTOOLNIX_URL
  !define MUI_DIRECTORYPAGE_TEXT_TOP "Setup downloads MKVToolNix from its official site. XFold works offline after installation."
!endif
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetRegView 64
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
!ifdef MKVTOOLNIX_URL
  ${IfNot} ${FileExists} "$SYSDIR\tar.exe"
  ${OrIfNot} ${FileExists} "$SYSDIR\curl.exe"
    MessageBox MB_OK|MB_ICONSTOP "Setup requires the Windows curl and tar tools. Use Windows 10 version 1809 or newer." /SD IDOK
    SetErrorLevel 10
    Quit
  ${EndIf}
!endif
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "XFold"
  SetShellVarContext all
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
!ifdef MKVTOOLNIX_URL
    DetailPrint "Preparing and checking MKVToolNix..."
    InitPluginsDir
    File /oname=$PLUGINSDIR\fetch-mkvtoolnix.ps1 "fetch-mkvtoolnix.ps1"
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\fetch-mkvtoolnix.ps1" -ArchiveUri "${MKVTOOLNIX_URL}" -Sha256 "${MKVTOOLNIX_SHA256}" -Destination "$INSTDIR\pending\tools\bin\mkvtoolnix" -LocalArchive "$EXEDIR\${MKVTOOLNIX_FILENAME}"'
    Pop $0
    Pop $1
    StrCmp $0 "0" 0 download_failed
!endif
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
    WriteRegStr HKLM "Software\XFold" "InstallDir" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "DisplayName" "XFold"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "InstallLocation" "$INSTDIR"
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "NoModify" 1
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold" "NoRepair" 1
    RMDir /r "$INSTDIR\previous"
    Goto done

!ifdef MKVTOOLNIX_URL
  download_failed:
    SetOutPath "$INSTDIR"
    RMDir /r "$INSTDIR\pending"
    DetailPrint "$1"
    MessageBox MB_OK|MB_ICONSTOP "Setup could not install MKVToolNix. Check your connection, or download the official ${MKVTOOLNIX_FILENAME} to the folder containing Setup and retry.$\r$\n$\r$\n$1" /SD IDOK
    SetErrorLevel 14
    ${If} $0 >= 11
    ${AndIf} $0 <= 13
      SetErrorLevel $0
    ${EndIf}
    Quit
!endif

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
  SetShellVarContext all
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
  ReadRegStr $0 HKLM "Software\XFold" "InstallDir"
  ${If} $0 == "$INSTDIR"
    Delete "$SMPROGRAMS\XFold\XFold.lnk"
    Delete "$SMPROGRAMS\XFold\Uninstall XFold.lnk"
    RMDir "$SMPROGRAMS\XFold"
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold"
    DeleteRegValue HKLM "Software\XFold" "InstallDir"
    DeleteRegKey /ifempty HKLM "Software\XFold"
  ${EndIf}
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Goto done
  missing:
    MessageBox MB_OK|MB_ICONSTOP "XFold could not be uninstalled. Close XFold and its tools, then retry." /SD IDOK
    SetErrorLevel 2
  done:
SectionEnd
