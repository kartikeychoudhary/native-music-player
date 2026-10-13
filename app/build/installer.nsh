; Included by electron-builder's installer script (nsis.include). On a real uninstall (never during an
; update, which runs the old uninstaller silently, and never in silent mode) offer to remove Aurora's
; library, playlists, settings and cover cache as well. Music files are never touched.

; electron-builder's APP_ASSOCIATE points each extension's default value at Aurora.Audio, and its
; APP_UNASSOCIATE leaves that default behind (a dangling class name), so a real uninstall clears it
; here, but only while it is still ours. Keep the extensions below in step with fileAssociations.ext in
; electron-builder.yml (app/test/installKind.test.ts checks).
!macro auroraClearDefault EXT
  ReadRegStr $R0 SHELL_CONTEXT "Software\Classes\.${EXT}" ""
  ${if} $R0 == "Aurora.Audio"
    DeleteRegValue SHELL_CONTEXT "Software\Classes\.${EXT}" ""
  ${endIf}
!macroend

!macro customUnInstall
  ${ifNot} ${isUpdated}
    ; This runs in the install's shell context (all users for a per-machine install), where the
    ; associations were written. An update re-registers them, so only a real uninstall clears them.
    !insertmacro auroraClearDefault "mp3"
    !insertmacro auroraClearDefault "m4a"
    !insertmacro auroraClearDefault "m4b"
    !insertmacro auroraClearDefault "aac"
    !insertmacro auroraClearDefault "flac"
    !insertmacro auroraClearDefault "wav"
    !insertmacro auroraClearDefault "aif"
    !insertmacro auroraClearDefault "aiff"
    !insertmacro auroraClearDefault "aifc"
    !insertmacro auroraClearDefault "ogg"
    !insertmacro auroraClearDefault "oga"
    !insertmacro auroraClearDefault "opus"
    !insertmacro auroraClearDefault "dsf"
    !insertmacro auroraClearDefault "dff"

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
