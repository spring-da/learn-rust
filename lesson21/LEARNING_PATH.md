# 第 21～23 课批量学习入口

| 课程 | 主题 | 入口 |
| --- | --- | --- |
| 21 | package、crate、module、可见性与公开 API | E:\WorkProject\rust\lesson21\README.md |
| 22 | 对照测试、生成数据、稳定版基准与优化判断 | E:\WorkProject\rust\lesson22\README.md |
| 23 | FnMut + Send 任务池、取消、重试、关闭、终态报表 | E:\WorkProject\rust\lesson23\README.md |

对应的 Go 项目在 E:\WorkProject\go 的同名 lesson21～lesson23 目录。
完整的配套阅读路线也可查看 E:\WorkProject\go\lesson21\LEARNING_PATH.md。

```powershell
& 'E:\WorkProject\rust\lesson21\run-batch.ps1'
```

脚本测试并运行三课，-Bench 可继续运行第 22 课的 release 基准。
各课只使用标准库，可离线构建。第 23 课会生成/覆盖其 data/results.tsv，导出的内容是终态结果，运行中闭包不在文件中。

建议每课先预测输出，运行后只挑一个条件修改；第 23 课再尝试独立重写调度核心，并使用已有测试检查边界。
