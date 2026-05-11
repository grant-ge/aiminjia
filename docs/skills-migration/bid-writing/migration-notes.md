# bid-writing 技能迁移说明

## 来源

新建技能（非迁移），随小标员工同时上线。

## 设计参考

- 设计 spec: `lotus/docs/superpowers/specs/2026-05-08-new-employee-xiaobiao-bid-writer.md`
- 同构参考: `resume-screening`（小招技能，同样是按需触发 + 文件输入 + 多工具协作）

## 与已有技能的边界

- `biz-proposal`：商业方案撰写（创意性强、结构灵活），输出文本/PPT
- `bid-writing`：投标文件撰写（强结构、强对齐招标要求），输出 docx
- 二者不互替；用户雇佣"小标"时 default skill 锁定为 `bid-writing`

## 资源依赖

- `execute_python`：模板解析依赖 `python-docx`（已在 `requirements.txt`）
- `generate_report` (`format: 'docx'`)：最终 docx 导出
- `web_search` / `browse_and_extract`：行业信息补充（可关）

## 测试约束

发布前烟测：拖入一份示例招标书 PDF + DOCX 模板，完成 4 步工作流并导出非空 docx。
