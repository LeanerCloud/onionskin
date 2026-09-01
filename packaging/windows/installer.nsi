; NSIS installer for Onionskin. Build with:
;   makensis -DVERSION=0.1.0 packaging/windows/installer.nsi
; Expects target\release\onionskin.exe from:
;   cargo build --release -p onionskin-app --features shell
!ifndef VERSION
  !error "VERSION is required: makensis -DVERSION=x.y.z installer.nsi"
!endif

Name "Onionskin ${VERSION}"
OutFile "..\..\dist\Onionskin-${VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\Onionskin"
InstallDirRegKey HKLM "Software\Onionskin" "InstallDir"
RequestExecutionLevel admin
Unicode true

; No Icon/UninstallIcon directives: the icon set does not exist yet, so NSIS
; uses its defaults. See packaging/README.md.

; Registered under the extension's OpenWithProgids rather than as its default
; handler, so installing Onionskin never silently takes PDFs off Acrobat. It
; joins the "Open with" menu and Windows offers it as a choice.
!macro AssociateExt ext desc
  WriteRegStr HKCR ".${ext}\OpenWithProgids" "Onionskin.${ext}" ""
  WriteRegStr HKCR "Onionskin.${ext}" "" "${desc}"
  WriteRegStr HKCR "Onionskin.${ext}\shell\open\command" "" '"$INSTDIR\onionskin.exe" "%1"'
!macroend

!macro UnassociateExt ext
  DeleteRegValue HKCR ".${ext}\OpenWithProgids" "Onionskin.${ext}"
  DeleteRegKey HKCR "Onionskin.${ext}"
!macroend

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Onionskin"
  SetOutPath "$INSTDIR"
  File "..\..\target\release\onionskin.exe"
  File "..\..\LICENSE"

  WriteRegStr HKLM "Software\Onionskin" "InstallDir" "$INSTDIR"
  ; Add/Remove Programs entry.
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Onionskin" \
    "DisplayName" "Onionskin"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Onionskin" \
    "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Onionskin" \
    "UninstallString" "$INSTDIR\uninstall.exe"

  !insertmacro AssociateExt "pdf" "PDF Document"
  ; SHCNE_ASSOCCHANGED: pick the association up now rather than at the next
  ; sign-in, so the Open With menu is right immediately.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'

  ; The installer runs elevated and writes to HKLM, so the shortcut belongs
  ; in the all-users Start Menu, not in whatever profile ran it.
  SetShellVarContext all
  CreateShortcut "$SMPROGRAMS\Onionskin.lnk" "$INSTDIR\onionskin.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  ; Same context the install used, or $SMPROGRAMS resolves elsewhere and the
  ; shortcut is left behind.
  SetShellVarContext all
  Delete "$INSTDIR\onionskin.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\Onionskin.lnk"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\Onionskin"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Onionskin"
  !insertmacro UnassociateExt "pdf"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd
