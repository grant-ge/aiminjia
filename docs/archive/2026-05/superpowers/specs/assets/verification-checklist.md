# Plan-E 视觉对照清单

执行方式：
1. `pnpm tauri:dev` 启动应用
2. 逐一切换到下方 10 个页面
3. 用 `pnpm capture:ui` 或手工截图到 `tmp/ui-capture/<name>.png`
4. 与 `docs/superpowers/specs/assets/design-pen-exports/<name>.png` 对比
5. 在每页 3 个检查点上打 ✓ / ✗，✗ 必须附记问题与修复 commit hash

| # | 页面 | 稿 | 检查点 1 | 检查点 2 | 检查点 3 | 状态 | 备注 |
|---|---|---|---|---|---|---|---|
| 1 | 首页 | home.png | ☐ mascot 64 圆居中标题上 | ☐ "为你推荐" chip 金底激活含 sparkles | ☐ 三态行卡 iconBox 底色按 variant | - | |
| 2 | 聊天长对话 | chat-long.png | ☐ 用户气泡金底右对齐 max 80% | ☐ ToolGroup 顶栏绿 check + 已完成 N 步 + 时长 | ☐ GeneratedFileCard 右侧 app pill | - | |
| 3 | 聊天技能弹层 | chat-skill-popover.png | ☐ popover 锚在 composer 上方 | ☐ 头部 "管理已安装的技能" | ☐ 行 padding 左右标题/标签 | - | |
| 4 | 技能中心 | skill-center.png | ☐ TopBar 右上 "技能市场/上传技能" | ☐ "热门推荐" 15/600 + 网格 gap 16 | ☐ "办公效率" 分类条 + 网格 | - | |
| 5 | 技能详情 | skill-detail.png | ☐ heroIc 88×88 底 brand-primary-subtle | ☐ meta 行 gap 48（来源/更新时间） | ☐ 右上"禁用 outline" + "使用 primary" 按钮组 | - | |
| 6 | 定时任务 | schedules.png | ☐ 3 张模板卡 padding 18 gap 16 | ☐ 列表卡 header padding [16,20] | ☐ 空态居中 h 280 | - | |
| 7 | 设置账户 | settings-account.png | ☐ Modal 980×680 居中 + 遮罩 | ☐ 左 220 menu "账户" 激活白底 | ☐ 账户卡 secondary 底 r-14 退出按钮 outline | - | |
| 8 | 设置关于 | settings-about.png | ☐ "关于 AI 小家" 激活 | ☐ appCard 平铺 padding 20 | ☐ 帮助/开发者两段 gap 16 | - | |
| 9 | 设置用量 | settings-usage.png | ☐ "用量" 激活 | ☐ planCard 底 border 1 | ☐ quota 进度条 + detail 列 | - | |
| 10 | 登录 | login.png | ☐ logo 56 圆 + brand 22/600 | ☐ Card 460 r-18 padding [40,40,32,40] | ☐ 登录按钮 r-999 金底 fontSize 15/600 | - | |

完成标准：所有 30 项 ✓ 视为本轮 plan-E DoD 通过。
