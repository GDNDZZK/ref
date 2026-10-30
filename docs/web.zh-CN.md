[English](web.md) · **简体中文**

# Web 界面

- [启动](#启动)
- [登录](#登录)
- [每个端口一个实例](#每个端口一个实例)
- [向局域网开放](#向局域网开放)
- [通知](#通知)
- [开发页面](#开发页面)

`ref-web` 在浏览器中显示 ref。它是一个小型 Web 服务器：提供页面、为浏览器登录，并把浏览器的 WebSocket 帧原样转发给 `ref-server`。会话、工具和权限都留在后端，所以同一个会话可以同时在终端和浏览器标签页中打开。与 `ref` 一样，`ref-web` 通过 instance 文件找到运行中的后端，没有时启动同目录的 `ref-server`；这样启动的后端会在最后一个客户端断开后退出。

## 启动

```bash
ref-web                       # 监听 127.0.0.1:7480 并打开浏览器
ref-web ~/src/project         # 新会话在此目录启动
ref-web --port 8080 --no-open # 换端口，不打开浏览器
```

全部选项见[命令行](cli.zh-CN.md#ref-web)。页面把你传入的目录作为新会话的目录；没有传入时使用 `ref-web` 的启动目录。

## 登录

启动时，`ref-web` 会打印登录地址并在浏览器中打开：

```
http://127.0.0.1:7480/login?token=…&dir=/home/you/src/project
```

打开这个地址会写入 cookie（`HttpOnly`、`SameSite=Strict`，有效期 30 天）并跳转到页面，因此 token 不会留在地址栏和浏览器历史中。之后在 cookie 过期前，直接打开 `http://127.0.0.1:7480/` 即可，不需要 token。

token 在首次启动时生成，保存在 `<state 目录>/web/login-token`（权限 600），所以重启后登录地址不变。要换新的 token，先停止 `ref-web`，删除这个文件，再重新启动。用旧 token 登录的浏览器会退出登录。

## 每个端口一个实例

每个端口只运行一个 `ref-web`。该端口已有实例时再次启动，会打印登录地址（带本次的 `DIR`）并打开浏览器，然后以退出码 0 退出。已运行的实例保持它自己的 `--server`、`--mock` 和 `--host`，第二次启动的这些选项不生效。端口被其他程序占用时，`ref-web` 报错并以退出码 1 退出。

## 向局域网开放

`ref-web` 默认只监听 `127.0.0.1`。`--host ADDR` 可以监听其他地址，例如 `--host 0.0.0.0` 向局域网开放，此时会打印警告。

- 请用 **IP 地址**访问，例如 `http://192.168.1.20:7480/`。用主机名访问会被拒绝：`ref-web` 会用已知地址校验 `Host` 头，以防范 DNS rebinding。
- 没有 TLS。cookie 和你输入的所有内容都以明文传输，登录的人可以控制后端，而后端能在这台机器上运行命令。请改用 SSH 隧道，或带 TLS 的反向代理：

  ```bash
  ssh -L 7480:127.0.0.1:7480 server
  # 然后在本机打开 http://127.0.0.1:7480/
  ```

要使用在别处运行的后端，传入 `--server ws://host:port --token T`（见[在另一台机器上运行后端](cli.zh-CN.md#在另一台机器上运行后端)）。`--token` 必须与 `--server` 一起使用，`--server` 不能与 `--mock` 同时使用。

## 通知

页面可以通过浏览器显示系统通知。浏览器只在 `localhost`、`127.0.0.1` 和 `https` 页面允许通知，所以通过局域网中的普通 `http` 地址访问时，只有标签页标题和标签页图标会提示需要处理。

## 开发页面

页面在 `web/` 中（Vue 3、TypeScript、Vite、pnpm）。`ref-web` 嵌入构建后的文件，构建方法见[开发](development.zh-CN.md#构建)。

```bash
cd web
pnpm install
pnpm dev                  # Vite 开发服务器，127.0.0.1:5173
```

`pnpm dev` 把 `/ws`、`/api` 和 `/login` 转发到 `127.0.0.1:7480` 上的 `ref-web`。先启动 `ref-web --no-open`（或 `ref-web --mock --no-open`），再用登录地址中的 token 通过开发服务器登录：`http://127.0.0.1:5173/login?token=…`。全程使用 `127.0.0.1` 而不是 `localhost`，因为 cookie 属于某一个主机名。
