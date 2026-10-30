**English** · [简体中文](web.zh-CN.md)

# Web UI

- [Starting](#starting)
- [Signing in](#signing-in)
- [One instance per port](#one-instance-per-port)
- [Opening it to the network](#opening-it-to-the-network)
- [Notifications](#notifications)
- [Developing the page](#developing-the-page)

`ref-web` shows ref in a browser. It is a small web server: it serves the page, signs the browser in, and forwards the browser's WebSocket frames unchanged to `ref-server`. Sessions, tools and permissions all stay in the backend, so a session can be open in the terminal and in a browser tab at the same time. Like `ref`, `ref-web` finds a running backend through the instance files and otherwise starts the `ref-server` next to it; a backend started this way exits when the last client disconnects.

## Starting

```bash
ref-web                       # serve on 127.0.0.1:7480 and open the browser
ref-web ~/src/project         # new sessions start in this directory
ref-web --port 8080 --no-open # another port, do not open the browser
```

All options are listed in [Command line](cli.md#ref-web). The page uses the directory you pass as the directory for new sessions; without one, it uses the directory `ref-web` was started in.

## Signing in

On start, `ref-web` prints a login address and opens it in the browser:

```
http://127.0.0.1:7480/login?token=…&dir=/home/you/src/project
```

The address sets a cookie (`HttpOnly`, `SameSite=Strict`, valid for 30 days) and redirects to the page, so the token does not stay in the address bar or the browser history. After that, `http://127.0.0.1:7480/` works without the token until the cookie expires.

The token is created on first start and kept in `<state dir>/web/login-token` (mode 600), so the login address stays the same after a restart. To get a new token, stop `ref-web`, delete that file and start it again. Browsers signed in with the old token are signed out.

## One instance per port

Only one `ref-web` runs per port. Starting it again while one is running on that port prints the login address (with the new `DIR`) and opens the browser, then exits with code 0. The running instance keeps its own `--server`, `--mock` and `--host`; the options of the second start are not applied. If another program uses the port, `ref-web` reports the error and exits with code 1.

## Opening it to the network

`ref-web` listens on `127.0.0.1` only. `--host ADDR` listens on another address, for example `--host 0.0.0.0` for the local network. It prints a warning when you do.

- Open the page by **IP address**, for example `http://192.168.1.20:7480/`. A host name is rejected: `ref-web` checks the `Host` header against the addresses it knows to prevent DNS rebinding.
- There is no TLS. The cookie and everything you type travel in plain text, and whoever signs in controls the backend, which can run commands on that machine. Use an SSH tunnel or a reverse proxy with TLS instead:

  ```bash
  ssh -L 7480:127.0.0.1:7480 server
  # then open http://127.0.0.1:7480/ on your machine
  ```

To use a backend that runs somewhere else, pass `--server ws://host:port --token T` (see [Running the backend on another machine](cli.md#running-the-backend-on-another-machine)). `--token` needs `--server`, and `--server` cannot be combined with `--mock`.

## Notifications

The page can show system notifications through the browser. Browsers allow them only on `localhost`, `127.0.0.1` and `https` pages, so on a plain `http` address on the network only the tab title and the tab icon show that something needs attention.

## Developing the page

The page is in `web/` (Vue 3, TypeScript, Vite, pnpm). `ref-web` embeds the built files, so see [Development](development.md#building) for how to build.

```bash
cd web
pnpm install
pnpm dev                  # Vite dev server on 127.0.0.1:5173
```

`pnpm dev` forwards `/ws`, `/api` and `/login` to `ref-web` on `127.0.0.1:7480`. Start `ref-web --no-open` (or `ref-web --mock --no-open`) first, then sign in through the dev server with `http://127.0.0.1:5173/login?token=…`, using the token from the login address. Use `127.0.0.1` throughout, not `localhost`, because the cookie belongs to one host name.
