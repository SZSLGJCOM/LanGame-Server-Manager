# SCUM 玩家读取扩展

`LgsmPlayerQuery` 从服务器当前连接中读取玩家名称和会话 ID。名单只读，不提供踢出、封禁或任意命令接口。原生 A2S 在已测试版本中没有响应，LanGame 改用此扩展；扩展缺失或采集不完整时显示错误，不把失败当成零人。

已验证 SCUM build `24973389`、Unreal Engine `4.27.2` 和官方 UE4SS `3.0.1-1125-g527a483b`。真实空服和构造行为测试的范围见 [SOURCE.md](SOURCE.md)。真实多人加入、离开和重连尚未验收。

## 安装

停止所有使用目标安装目录的实例。以下安装器仅适用于没有既有加载器的目录；有其他模组时使用独立安装目录，不直接覆盖原配置。

准备官方资产 `UE4SS_v3.0.1-1125-g527a483b.zip`，然后从仓库根目录运行：

```powershell
.\modules\scum\extensions\install-loader.ps1 `
    -ServerRoot (Read-Host 'SCUM 专服安装目录完整路径') `
    -ArchivePath (Read-Host '官方 UE4SS 1125 ZIP 完整路径') `
    -ExtractionDirectory (Read-Host '仓库和专服目录外的新解压目录完整路径')
```

安装器校验 ZIP 和核心 DLL 的 SHA-256，保留上游许可证，只安装官方 `dwmapi.dll`、核心、专用配置及本项目的查询脚本，不安装默认调试模组。固定资产名和哈希见 [SOURCE.md](SOURCE.md)；上游实验发布页会移动，不能直接用页面上的其他版本替代。

在 LanGame 中启动实例，等待世界加载完成，然后刷新玩家选项卡。SCUM 原生程序仍需要 Windows 提权。后续启动准备会更新本项目脚本，不下载 DLL，也不覆盖已有加载器配置。

## 排查

查看 `SCUM/Binaries/Win64/ue4ss/UE4SS.log`，确认 `LgsmPlayerQuery` 已加载且没有 Lua 错误。请求和响应位于安装根目录的 `langame_player_query/`。加载世界期间可能返回未就绪；停服后的旧响应不能用作在线名单。

游戏和加载器更新后应重新验证。不能使用加入、退出日志、管理员名单或数据库中的历史玩家记录拼接在线名单。会话 ID 不是 Steam64 ID，不能用于账号管理。
