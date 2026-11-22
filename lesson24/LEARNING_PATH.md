# 第 24～26 课批量学习入口

本批依次实现持久化任务存储、延迟重试调度、HTTP 服务。
lesson25 通过 path 依赖 lesson24；lesson26 依赖两者。三个目录应一起保留。

```powershell
& 'E:\WorkProject\rust\lesson24\run-batch.ps1' -UseMirror
```

- -UseMirror：命令级 rsproxy 配置，不修改全局 Cargo 配置；已有可用源可省略。
- -Offline：依赖已下载时离线运行。
- 脚本逐课测试并演示；第 26 课自动启动、验证、重启并停止，不留下常驻服务。

首次会下载 bundled SQLite、Axum/Tokio 等依赖；当前环境已完成编译和验证。
对应 Go 项目位于 E:\WorkProject\go 的 lesson24～lesson26。
完整路线与验证记录见 E:\WorkProject\go\lesson24\LEARNING_PATH.md。

建议先从 24 的 Task/Spec 与条件更新开始，随后在 25 人工时钟中推导到期顺序，最后在 26 观察真实 HTTP 与后台线程协作。
单 owner 互斥、尝试预算、取消持久化、执行令牌，以及跨语言数据库接续，都已经过实际验证。
