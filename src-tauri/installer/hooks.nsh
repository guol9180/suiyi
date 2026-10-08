; 安装完成后的收尾动作。
;
; 为什么需要：Windows 会把桌面快捷方式与任务栏按钮的图标缓存起来。升级安装换了带新图标的
; exe 之后，用户看到的常常还是旧图标，得手动清缓存或重启资源管理器才刷新。
; 装完顺手刷一次，新装 / 升级的用户都不用再管这件事。
;
; 注意：任务栏「已钉住」的那一项由注册表缓存，程序改不了，那一项仍需用户取消固定再钉一次。

!macro NSIS_HOOK_POSTINSTALL
  ; ie4uinit -show 是 Windows 自带的图标缓存刷新命令（静默执行，不弹窗）
  nsExec::ExecToLog '"$SYSDIR\ie4uinit.exe" -show'
  Pop $0
  ; 再广播一次 shell 关联变更：SHCNE_ASSOCCHANGED = 0x08000000，SHCNF_IDLIST = 0
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend
