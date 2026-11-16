; -*- coding: utf-8 -*-
Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "FileFunc.nsh"

!ifndef APP_ID
  !define APP_ID "AuraIsle"
!endif
!ifndef APP_NAME
  !define APP_NAME "灵屿 AuraIsle"
!endif
!define LEGACY_APP_ID "Auralsle"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
!define LEGACY_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${LEGACY_APP_ID}"
Name "${APP_NAME}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_ID}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
ManifestDPIAware true
CRCCheck on
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "CompanyName" "Eternal3240 / 灵屿 AuraIsle"
VIAddVersionKey /LANG=2052 "ProductName" "${APP_NAME}"
VIAddVersionKey /LANG=2052 "FileDescription" "${APP_NAME} 安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "OriginalFilename" "AuraIsle-Setup-v${APP_VERSION}-x64.exe"
VIAddVersionKey /LANG=2052 "LegalCopyright" "灵屿 AuraIsle 贡献者"
!define MUI_ICON "${PROJECT_DIR}\assets\auralsle.ico"
!define MUI_UNICON "${PROJECT_DIR}\assets\auralsle.ico"
!define MUI_ABORTWARNING
Var BrandIcon
Var PathInput
Var DesktopCheckbox
Var AutoStartCheckbox
Var DesktopChoice
Var AutoStartChoice
Page custom InstallPage InstallPageLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "灵屿 AuraIsle 已安装"
!define MUI_FINISHPAGE_TEXT "音乐歌词、桌面提醒、AI 状态与实用工具，现在都在屏幕顶部的信息岛里。"
!define MUI_FINISHPAGE_RUN "$INSTDIR\AuraIsle.exe"
!define MUI_FINISHPAGE_RUN_TEXT "打开灵屿 AuraIsle"
!insertmacro MUI_PAGE_FINISH
!define MUI_UNCONFIRMPAGE_TEXT_TOP "将卸载灵屿 AuraIsle。设置、待办和历史数据仍保存在本机用户目录中，重新安装后可继续使用。"
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
BrandingText "灵屿 · AuraIsle"
SetFont "Microsoft YaHei UI" 9

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "灵屿 AuraIsle 需要 Windows 10 或 Windows 11 的 64 位系统。" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "灵屿 AuraIsle 需要 Windows 10 或 Windows 11。" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  StrCpy $DesktopChoice 1
  StrCpy $AutoStartChoice 0
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != ""
    IfFileExists "$DESKTOP\${APP_NAME}.lnk" +2 0
      StrCpy $DesktopChoice 0
  ${EndIf}
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 != ""
    StrCpy $AutoStartChoice 1
!if "${APP_ID}" == "AuraIsle"
  ${Else}
    ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${LEGACY_APP_ID}"
    ${If} $0 != ""
      StrCpy $AutoStartChoice 1
    ${EndIf}
!endif
  ${EndIf}
  ; 静默安装使用同一组选项；/D 路径由 NSIS 自身处理。
  ${GetParameters} $0
  ${GetOptions} $0 "/DESKTOP=" $1
  ${If} $1 == "0"
  ${OrIf} $1 == "1"
    StrCpy $DesktopChoice $1
  ${EndIf}
  ${GetOptions} $0 "/AUTOSTART=" $1
  ${If} $1 == "0"
  ${OrIf} $1 == "1"
    StrCpy $AutoStartChoice $1
  ${EndIf}
FunctionEnd

Function InstallPage
  !insertmacro MUI_HEADER_TEXT "欢迎使用灵屿 AuraIsle" "Windows 顶部的桌面伙伴与智能信息岛"
  nsDialogs::Create 1018
  Pop $0
  InitPluginsDir
  File /oname=$PLUGINSDIR\brand.ico "${PROJECT_DIR}\assets\auralsle.ico"
  ${NSD_CreateIcon} 0 6u 44u 44u ""
  Pop $0
  System::Call 'user32::LoadImageW(p 0, w "$PLUGINSDIR\brand.ico", i 1, i 64, i 64, i 0x10) p .r1'
  StrCpy $BrandIcon $1
  SendMessage $0 0x170 $1 0
  ${NSD_CreateLabel} 58u 8u 240u 22u "灵屿 AuraIsle"
  Pop $0
  CreateFont $1 "Microsoft YaHei UI" 18 700
  SendMessage $0 ${WM_SETFONT} $1 0
  ${NSD_CreateLabel} 58u 30u 240u 14u "v${APP_VERSION} · 升级保留全部设置和历史数据"
  Pop $0
  ${NSD_CreateLabel} 0 58u 300u 16u "音乐歌词 · 硬件监控 · 剪贴板 · AI 状态 · 日程与天气"
  Pop $0
  ${NSD_CreateLabel} 0 80u 300u 12u "安装位置"
  Pop $0
  ${NSD_CreateDirRequest} 0 96u 245u 16u "$INSTDIR"
  Pop $PathInput
  ${NSD_CreateBrowseButton} 250u 96u 50u 16u "浏览…"
  Pop $0
  ${NSD_OnClick} $0 BrowseDirectory
  ${NSD_CreateCheckbox} 0 122u 150u 14u "创建桌面快捷方式"
  Pop $DesktopCheckbox
  ${NSD_SetState} $DesktopCheckbox $DesktopChoice
  ${NSD_CreateCheckbox} 165u 122u 135u 14u "开机自启"
  Pop $AutoStartCheckbox
  ${NSD_SetState} $AutoStartCheckbox $AutoStartChoice
  GetDlgItem $0 $HWNDPARENT 1
  SendMessage $0 ${WM_SETTEXT} 0 "STR:一键安装"
  nsDialogs::Show
FunctionEnd

Function CheckFolderPath
  ${GetRoot} "$INSTDIR" $0
  ${If} $0 != ""
    StrLen $1 $0
    StrCpy $2 "$INSTDIR" "" $1
    StrCpy $1 $2 1
    ${If} $1 == "\"
    ${AndIf} $2 != "\"
      System::Call 'kernel32::GetFullPathNameW(w "$INSTDIR", i ${NSIS_MAX_STRLEN}, w .r2, p 0) i .r0'
      ${If} $0 == 0
      ${OrIf} $0 >= ${NSIS_MAX_STRLEN}
        Goto invalid_path
      ${EndIf}
      System::Call 'shlwapi::PathIsRootW(w "$2") i .r0'
      ${If} $0 == 0
        StrCpy $INSTDIR $2
        StrCpy $0 1
        Return
      ${EndIf}
    ${EndIf}
  ${EndIf}
  invalid_path:
  StrCpy $0 0
FunctionEnd

Function EnsureOwnFolder
  Call CheckFolderPath
  ${If} $0 == 0
    Return
  ${EndIf}
  StrCpy $0 $INSTDIR 1 -1
  ${If} $0 == "\"
    StrCpy $INSTDIR $INSTDIR -1
  ${EndIf}
  ${GetFileName} "$INSTDIR" $0
  ${If} $0 == "${APP_ID}"
  ${OrIf} $0 == "${LEGACY_APP_ID}"
    Return
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\AuraIsle.exe"
  ${OrIf} ${FileExists} "$INSTDIR\auralsle.exe"
    Return
  ${EndIf}
  ClearErrors
  FindFirst $1 $2 "$INSTDIR\*.*"
  ${DoWhile} $2 != ""
    ${If} $2 != "."
    ${AndIf} $2 != ".."
      FindClose $1
      StrCpy $INSTDIR "$INSTDIR\${APP_ID}"
      Return
    ${EndIf}
    FindNext $1 $2
  ${Loop}
  FindClose $1
  ClearErrors
FunctionEnd

Function BrowseDirectory
  Pop $0
  ${NSD_GetText} $PathInput $0
  nsDialogs::SelectFolderDialog "选择灵屿 AuraIsle 安装文件夹" "$0"
  Pop $0
  ${If} $0 != "error"
    StrCpy $INSTDIR $0
    Call EnsureOwnFolder
    ${NSD_SetText} $PathInput "$INSTDIR"
  ${EndIf}
FunctionEnd

Function ValidateDirectory
  Call CheckFolderPath
  ${If} $0 == 0
    Goto invalid
  ${EndIf}
  ClearErrors
  CreateDirectory "$INSTDIR"
  IfErrors invalid
  GetFullPathName $INSTDIR "$INSTDIR"
  IfErrors invalid
  GetTempFileName $0 "$INSTDIR"
  IfErrors invalid
  Delete "$0"
  SetErrorLevel 0
  Return
  invalid:
    MessageBox MB_OK|MB_ICONEXCLAMATION "请选择可写入的完整安装文件夹，例如 D:\Apps\AuraIsle。不要选择磁盘根目录或需要管理员权限的文件夹。" /SD IDOK
    SetErrorLevel 1
    Abort
FunctionEnd

Function InstallPageLeave
  ${NSD_GetText} $PathInput $INSTDIR
  ${NSD_GetState} $DesktopCheckbox $DesktopChoice
  ${NSD_GetState} $AutoStartCheckbox $AutoStartChoice
  Call EnsureOwnFolder
  ${NSD_SetText} $PathInput "$INSTDIR"
  Call ValidateDirectory
FunctionEnd

Function .onGUIEnd
  System::Call 'user32::DestroyIcon(p $BrandIcon)'
FunctionEnd

!macro ClosedCheck Prefix
Function ${Prefix}CheckClosed
  retry:
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\${APP_ID}.SingleInstance") p .r0'
!if "${APP_ID}" == "AuraIsle"
  ${If} $0 == 0
    System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\${LEGACY_APP_ID}.SingleInstance") p .r0'
  ${EndIf}
!endif
  ${If} $0 != 0
    System::Call 'kernel32::CloseHandle(p r0)'
    MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "请先从系统托盘退出${APP_NAME}，然后点击重试。" /SD IDCANCEL IDRETRY retry
    SetErrorLevel 2
    Abort
  ${EndIf}
FunctionEnd
!macroend
!insertmacro ClosedCheck ""
!insertmacro ClosedCheck "un."

Section "安装"
  Call CheckClosed
  Call EnsureOwnFolder
  Call ValidateDirectory
  SetOutPath "$INSTDIR"
  SetOverwrite on
  Delete "$INSTDIR\auralsle.exe"
  Delete "$INSTDIR\daozai.exe"
  File /oname=AuraIsle.exe "${SOURCE_EXE}"
  File "${PROJECT_DIR}\LICENSE"
  File "${PROJECT_DIR}\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\收集岛仔诊断.cmd"
  Delete "$INSTDIR\scripts\collect-diagnostics.ps1"
  RMDir "$INSTDIR\scripts"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  ${If} $DesktopChoice == 1
    CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\AuraIsle.exe" "" "$INSTDIR\AuraIsle.exe" 0
  ${EndIf}
  Delete "$DESKTOP\岛仔.lnk"
  ${If} $AutoStartChoice == 1
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}" '"$INSTDIR\AuraIsle.exe"'
  ${Else}
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${LEGACY_APP_ID}"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\灵屿设置.lnk"
  Delete "$SMPROGRAMS\收集灵屿诊断.lnk"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\AuraIsle.exe" "" "$INSTDIR\AuraIsle.exe" 0
  CreateShortcut "$SMPROGRAMS\灵屿设置.lnk" "$INSTDIR\AuraIsle.exe" "--settings" "$INSTDIR\AuraIsle.exe" 0
  CreateShortcut "$SMPROGRAMS\收集灵屿诊断.lnk" "$INSTDIR\AuraIsle.exe" "--collect-diagnostics" "$INSTDIR\AuraIsle.exe" 0
  Delete "$SMPROGRAMS\岛仔.lnk"
  Delete "$SMPROGRAMS\岛仔设置.lnk"
  Delete "$SMPROGRAMS\收集岛仔诊断.lnk"
  ; 通知 Windows Explorer / 任务栏立即刷新图标缓存，消除桌面与任务栏残留旧图标
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  IfFileExists "$SYSDIR\ie4uinit.exe" 0 +2
    ExecWait '"$SYSDIR\ie4uinit.exe" -show'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "灵屿 AuraIsle"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\AuraIsle.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/M=AuraIsle.exe /S=0K /G=0" $0 $1 $2
  ${GetSize} "$INSTDIR" "/M=Uninstall.exe /S=0K /G=0" $3 $1 $2
  IntOp $0 $0 + $3
  ${GetSize} "$INSTDIR" "/M=THIRD_PARTY_NOTICES.txt /S=0K /G=0" $3 $1 $2
  IntOp $0 $0 + $3
  IntOp $0 $0 + 4
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
  DeleteRegKey HKCU "${LEGACY_UNINSTALL_KEY}"
  Delete "$INSTDIR\uninstall.ps1"
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Section "Uninstall"
  Call un.CheckClosed
  Delete "$INSTDIR\AuraIsle.exe"
  Delete "$INSTDIR\auralsle.exe"
  Delete "$INSTDIR\AuraIsle-previous.exe"
  Delete "$INSTDIR\auralsle-previous.exe"
  Delete "$INSTDIR\auralsle-v0.2-backup.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\uninstall.ps1"
  Delete "$INSTDIR\收集岛仔诊断.cmd"
  Delete "$INSTDIR\scripts\collect-diagnostics.ps1"
  RMDir "$INSTDIR\scripts"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$DESKTOP\岛仔.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\灵屿设置.lnk"
  Delete "$SMPROGRAMS\收集灵屿诊断.lnk"
  Delete "$SMPROGRAMS\岛仔.lnk"
  Delete "$SMPROGRAMS\岛仔设置.lnk"
  Delete "$SMPROGRAMS\收集岛仔诊断.lnk"
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 == '"$INSTDIR\AuraIsle.exe"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${LEGACY_APP_ID}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${LEGACY_UNINSTALL_KEY}"
  RMDir "$INSTDIR"
SectionEnd
