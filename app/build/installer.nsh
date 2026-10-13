; Included by electron-builder's installer script (nsis.include). On a real uninstall (never during an
; update, which runs the old uninstaller silently, and never in silent mode) offer to remove Aurora's
; library, playlists, settings and cover cache as well. Music files are never touched.
!macro customUnInstall
  ${ifNot} ${isUpdated}
    MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your Aurora library, playlists, settings and cover cache?$\r$\n$\r$\nYour music files are not touched." /SD IDNO IDNO aurora_keep_data
      SetShellVarContext current
      RMDir /r "$APPDATA\Aurora"
    aurora_keep_data:
  ${endIf}
!macroend
