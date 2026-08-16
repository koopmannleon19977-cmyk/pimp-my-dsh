; pimp-my-dsh installer hook.
;
; Tauri's stock NSIS template creates the Start menu shortcut for every install
; mode but only auto-creates the desktop shortcut for silent/passive installers;
; for interactive installs it is otherwise an opt-in finish-page checkbox. This
; hook makes the desktop shortcut deterministic across GUI, silent, and passive
; installs by invoking the template's own helper (which already skips update and
; no-shortcut modes).
;
; installMode is "currentUser", so no elevation, no per-machine registry writes,
; and no autostart entry are performed by the installer.

!macro NSIS_HOOK_POSTINSTALL
  Call CreateOrUpdateDesktopShortcut
!macroend
