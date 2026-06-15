# Hosting the wasm client (GitHub Pages -> your dev server)

Goal: hand a URL to a dev; they open it in a browser and play against the server
running on your PC. The client is static-hosted on GitHub Pages (HTTPS); it
reaches your server over `wss://`.

```
dev browser ── https ──> GitHub Pages (wasm client)
     │
     └─ wss://game.idletree.net ──> Cloudflare Tunnel ──> 127.0.0.1:40001 (server)
```

Pages is HTTPS-only, and a browser on an HTTPS page can only open a **secure**
`wss://` socket. The server speaks plain `ws`, so something in front of it must
terminate TLS. We use **Cloudflare Tunnel**: no port-forwarding, hides your IP,
and `cloudflared` terminates TLS and proxies the WebSocket upgrade to the local
server. (DNS must be on Cloudflare.)

The server multiplexes JS5 + game on one port, so a single hostname covers both.

## Enable Pages (one time)
Repo **Settings -> Pages -> Build and deployment -> Source: "GitHub Actions"**.
`ci.yml` then runs the full `build.ps1` and deploys the wasm bundle it produced to
Pages on each push to master (PRs build+test only). Default URL:
`https://olden-shire.github.io/OS/`.

The deploy step bakes a default server into the page (`game.idletree.net`, or the
repo variable `OS_DEFAULT_SERVER` if set), so the URL you hand out is just:
```
https://olden-shire.github.io/OS/
```
On this HTTPS page the client connects with `wss://` automatically (`ws_socket.rs`).
A `?server=host` in the URL still overrides the default; local/dev builds (the
committed `index.html`, no inject) keep targeting `127.0.0.1`.

## Cloudflare Tunnel

Needs the domain's DNS on Cloudflare (Add a site -> set the given nameservers at
your registrar).
```powershell
winget install Cloudflare.cloudflared
cloudflared tunnel login
cloudflared tunnel create os
cloudflared tunnel route dns os game.idletree.net
```
`%USERPROFILE%\.cloudflared\config.yml`:
```yaml
tunnel: os
credentials-file: C:\Users\<you>\.cloudflared\<tunnel-id>.json
ingress:
  - hostname: game.idletree.net
    service: http://localhost:40001   # cloudflared proxies the WS upgrade
  - service: http_status:404
```
Run each session: `.\run-server.ps1` + `cloudflared tunnel run os`.

Quick throwaway test (no DNS setup): `cloudflared tunnel --url http://localhost:40001`
prints a `https://<random>.trycloudflare.com` - use it as `?server=<random>.trycloudflare.com`.

## Notes
- A custom Pages domain (e.g. `play.idletree.net` -> CNAME `olden-shire.github.io`,
  set in Pages settings) gives `https://play.idletree.net/?server=game.idletree.net`.
- Allow the server through the local firewall on :40001 (cloudflared reaches it on
  localhost, so usually fine).
