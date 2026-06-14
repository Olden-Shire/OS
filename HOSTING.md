# Hosting the wasm client (GitHub Pages -> your dev server)

Goal: hand a URL to a dev; they open it in a browser and play against the server
running on your PC. The client is static-hosted on GitHub Pages (HTTPS); it
reaches your server over `wss://`.

```
dev browser ── https ──> GitHub Pages (wasm client)
     │
     └─ wss://game.idletree.net ──> TLS terminator (Caddy or Cloudflare) ──> 127.0.0.1:40001 (server)
```

Pages is HTTPS-only, and a browser on an HTTPS page can only open a **secure**
`wss://` socket. The server speaks plain `ws`, so something in front of it must
terminate TLS. Two ways:

- **A. Caddy + Let's Encrypt (direct IP)** - DNS A record -> your IP, Caddy gets a
  real cert and reverse-proxies `wss`->`ws`. The active setup.
- **B. Cloudflare Tunnel** - no port-forwarding, hides your IP, but DNS must be on
  Cloudflare.

The server multiplexes JS5 + game on one port, so a single hostname covers both.

## Enable Pages (both options, one time)
Repo **Settings -> Pages -> Build and deployment -> Source: "GitHub Actions"**.
`pages.yml` then builds the wasm bundle and deploys it on each client-code push
(or "Run workflow"). Default URL: `https://olden-shire.github.io/OS/`.

The deploy step bakes a default server into the page (`game.idletree.net`, or the
repo variable `OS_DEFAULT_SERVER` if set), so the URL you hand out is just:
```
https://olden-shire.github.io/OS/
```
On this HTTPS page the client connects with `wss://` automatically (`ws_socket.rs`).
A `?server=host` in the URL still overrides the default; local/dev builds (the
committed `index.html`, no inject) keep targeting `127.0.0.1`.

## A. Caddy + Let's Encrypt (direct IP)

DNS A record `game.idletree.net` -> your public IP, **Cloudflare DNS-only (grey
cloud)** so Let's Encrypt can validate this origin (orange cloud intercepts 80/443
and serves Cloudflare's cert instead). Forward TCP **80 + 443** on the router to
this PC.

```powershell
winget install CaddyServer.Caddy
.\run-server.ps1                          # server on 0.0.0.0:40001
caddy run --config deploy\Caddyfile       # gets the cert, proxies wss -> :40001
```
`deploy/Caddyfile`:
```
game.idletree.net {
    reverse_proxy 127.0.0.1:40001
}
```
Caddy auto-obtains + renews the cert (HTTP-01 on :80 / TLS-ALPN on :443) and
upgrades WebSocket connections.

**Keep the orange cloud?** Use Caddy's DNS-01 challenge instead of opening :80:
install the Cloudflare-DNS build from caddyserver.com/download (module
`github.com/caddy-dns/cloudflare`), make a Cloudflare API token (Zone:DNS:Edit),
and use:
```
game.idletree.net {
    tls { dns cloudflare {env.CF_API_TOKEN} }
    reverse_proxy 127.0.0.1:40001
}
```
Then only :443 needs forwarding and the record can stay proxied.

## B. Cloudflare Tunnel (alternative, no port-forward)

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
- Allow the server through the local firewall on :40001 (Caddy/cloudflared reach it
  on localhost, so usually fine).
