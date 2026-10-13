; Included by electron-builder's installer script (nsis.include).

; Windows file types are registered here, not through electron-builder's fileAssociations: its
; APP_ASSOCIATE macro writes each extension's default value (Software\Classes\.<ext>), which silently makes
; Aurora the default player for anyone whose defaults are classic registry associations, and its
; uninstall cannot hand the default back. Aurora instead adds itself to Open with and to Settings >
; Default apps, and never touches an extension's default value; the user chooses it as default.
; (fileAssociations stays in electron-builder.yml under mac only.)
; app/test/installerAssoc.test.ts keeps the extension list below equal to mac.fileAssociations.ext and
; checks that no extension default is written.

; The one list of extensions. ACTION is the name of a macro that takes the extension.
!macro auroraForEachExt ACTION
  !insertmacro "${ACTION}" "mp3"
  !insertmacro "${ACTION}" "m4a"
  !insertmacro "${ACTION}" "m4b"
  !insertmacro "${ACTION}" "aac"
  !insertmacro "${ACTION}" "flac"
  !insertmacro "${ACTION}" "wav"
  !insertmacro "${ACTION}" "aif"
  !insertmacro "${ACTION}" "aiff"
  !insertmacro "${ACTION}" "aifc"
  !insertmacro "${ACTION}" "ogg"
  !insertmacro "${ACTION}" "oga"
  !insertmacro "${ACTION}" "opus"
  !insertmacro "${ACTION}" "dsf"
  !insertmacro "${ACTION}" "dff"
!macroend

; Offers Aurora in Open with for the extension (an empty REG_NONE value, as electron-builder's own
; FileAssociation.nsh writes it), and lists it as a supported type and a Default apps capability.
!macro auroraRegisterExt EXT
  WriteRegNone SHELL_CONTEXT "Software\Classes\.${EXT}\OpenWithProgids" "Aurora.Audio"
  WriteRegStr SHELL_CONTEXT "Software\Classes\Applications\${APP_EXECUTABLE_FILENAME}\SupportedTypes" ".${EXT}" ""
  WriteRegStr SHELL_CONTEXT "Software\Clients\Media\Aurora\Capabilities\FileAssociations" ".${EXT}" "Aurora.Audio"
!macroend

; Deletes a key only when it has no named values, no subkeys and an unset or empty (Default) value.
; Value index 0 may be an unnamed empty (Default) rather than a real value, so index 1 must be empty
; too: otherwise a key holding named values such as Content Type or PerceivedType would be deleted.
!macro auroraDeleteKeyIfEmpty KEY
  ClearErrors
  EnumRegValue $R1 SHELL_CONTEXT "${KEY}" 0
  ${if} ${Errors}
    StrCpy $R1 ""
  ${endIf}
  ClearErrors
  EnumRegValue $R4 SHELL_CONTEXT "${KEY}" 1
  ${if} ${Errors}
    StrCpy $R4 ""
  ${endIf}
  ClearErrors
  EnumRegKey $R2 SHELL_CONTEXT "${KEY}" 0
  ${if} ${Errors}
    StrCpy $R2 ""
  ${endIf}
  ClearErrors
  ReadRegStr $R3 SHELL_CONTEXT "${KEY}" ""
  ${if} $R1 == ""
  ${andIf} $R4 == ""
  ${andIf} $R2 == ""
  ${andIf} $R3 == ""
    DeleteRegKey SHELL_CONTEXT "${KEY}"
  ${endIf}
!macroend

!macro auroraUnregisterExt EXT
  DeleteRegValue SHELL_CONTEXT "Software\Classes\.${EXT}\OpenWithProgids" "Aurora.Audio"
  ; Then the OpenWithProgids key and the extension key, if that left them empty (Aurora created them).
  ; A key that still holds anything stays.
  !insertmacro auroraDeleteKeyIfEmpty "Software\Classes\.${EXT}\OpenWithProgids"
  !insertmacro auroraDeleteKeyIfEmpty "Software\Classes\.${EXT}"
!macroend

; Earlier test builds wrote Aurora.Audio as the extension default (and electron-builder's uninstall left
; it dangling), so a real uninstall clears it, but only while it is still ours.
!macro auroraClearDefault EXT
  ReadRegStr $R0 SHELL_CONTEXT "Software\Classes\.${EXT}" ""
  ${if} $R0 == "Aurora.Audio"
    DeleteRegValue SHELL_CONTEXT "Software\Classes\.${EXT}" ""
  ${endIf}
!macroend

; Runs in the install section, in the install's shell context (HKCU per user, HKLM per machine). It runs
; on updates too, which is harmless: every write is idempotent.
!macro customInstall
  ; The class: what Open with runs, and what Default apps maps each extension to.
  WriteRegStr SHELL_CONTEXT "Software\Classes\Aurora.Audio" "" "Audio file"
  WriteRegStr SHELL_CONTEXT "Software\Classes\Aurora.Audio\DefaultIcon" "" "$appExe,0"
  WriteRegStr SHELL_CONTEXT "Software\Classes\Aurora.Audio\shell" "" "open"
  WriteRegStr SHELL_CONTEXT "Software\Classes\Aurora.Audio\shell\open" "" "Open with Aurora"
  WriteRegStr SHELL_CONTEXT "Software\Classes\Aurora.Audio\shell\open\command" "" `"$appExe" "%1"`

  ; The application registration, which Open with lists by its friendly name.
  WriteRegStr SHELL_CONTEXT "Software\Classes\Applications\${APP_EXECUTABLE_FILENAME}" "FriendlyAppName" "Aurora"
  WriteRegStr SHELL_CONTEXT "Software\Classes\Applications\${APP_EXECUTABLE_FILENAME}\shell\open\command" "" `"$appExe" "%1"`

  ; Settings > Default apps.
  WriteRegStr SHELL_CONTEXT "Software\Clients\Media\Aurora\Capabilities" "ApplicationName" "Aurora"
  WriteRegStr SHELL_CONTEXT "Software\Clients\Media\Aurora\Capabilities" "ApplicationDescription" "Hi-res music player"
  WriteRegStr SHELL_CONTEXT "Software\RegisteredApplications" "Aurora" "Software\Clients\Media\Aurora\Capabilities"

  !insertmacro auroraForEachExt auroraRegisterExt

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

; Every uninstall, including the silent one an update or an install-mode switch runs (a per-machine
; install replacing a per-user one runs the old per-user uninstaller with --updated), removes what
; customInstall added, so stale registrations never outlive the exe they point at. A normal update
; re-registers right afterwards, because the install section runs customInstall after the old version is
; uninstalled. Only a real uninstall (never an update, never silent) clears an extension default that is
; ours, which an update must not undo, and offers to remove Aurora's library, playlists, settings and
; cover cache. Music files are never touched.
!macro customUnInstall
  ; This runs in the install's shell context (all users for a per-machine install), where the
  ; registrations were written, so all of it is done before the context is switched below.
  ${ifNot} ${isUpdated}
    !insertmacro auroraForEachExt auroraClearDefault
  ${endIf}

  !insertmacro auroraForEachExt auroraUnregisterExt
  DeleteRegKey SHELL_CONTEXT "Software\Classes\Aurora.Audio"
  DeleteRegKey SHELL_CONTEXT "Software\Classes\Applications\${APP_EXECUTABLE_FILENAME}"
  DeleteRegKey SHELL_CONTEXT "Software\Clients\Media\Aurora"
  DeleteRegValue SHELL_CONTEXT "Software\RegisteredApplications" "Aurora"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  ${ifNot} ${isUpdated}
    MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your Aurora library, playlists, settings and cover cache?$\r$\n$\r$\nYour music files are not touched." /SD IDNO IDNO aurora_keep_data
      ; Electron's data is always per user. The rest of the uninstall section uses SHELL_CONTEXT for the
      ; uninstall key and associations, so the context goes back afterwards (as electron-builder's own
      ; --delete-app-data block does).
      ${if} $installMode == "all"
        SetShellVarContext current
      ${endIf}
      RMDir /r "$APPDATA\Aurora"
      ${if} $installMode == "all"
        SetShellVarContext all
      ${endIf}
    aurora_keep_data:
  ${endIf}
!macroend
