# Race Gateway WebUI

这是 `race-gateway` 管理端的前端源目录。

当前采用“源目录 + 嵌入静态资源”的轻量方案：

- `src/admin.html`
- `src/admin.css`
- `src/admin.js`

内嵌发布时对应复制到：

- `race-gateway/src/web/assets/`

这样可以保持：

- 前端源码与 Rust 嵌入资源分离
- admin 端口可直接独立提供页面
- 不额外引入 Node 构建链也能完成联调

