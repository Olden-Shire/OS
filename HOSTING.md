# Hosting the wasm client (GitHub Pages -> your dev server)

Goal: hand a URL to a dev; they open it in a browser and play against the server
running on your PC. The client is static-hosted on GitHub Pages; it reaches your
server over a Cloudflare Tunnel.

```
dev browser ── https ──> GitHub Pages (wasm client)
     │
     └─ wss://game.<domain> ──> Cloudflare edge ──> cloudflared (your PC) ──> 127.0.0.1:40001 (server)
```

Why the tunnel: Pages is HTTPS-only, and a browser on an HTTPS page can only open
a **secure** `wss://` socket. The server speaks plain `ws`, so Cloudflare
terminates TLS in front of it. The tunnel also means no port-forwarding and your
home IP stays private. The server multiplexes JS5 + game on one port, so a single
tunnel hostname covers both.

## 1. Enable Pages (one time)
Repo **Settings -> Pages -> Build and deployment -> Source: "GitHub Actions"**.
Then `pages.yml` builds the wasm bundle and deploys it on each push (client code)
or via "Run workflow". Default URL: `https://olden-shire.github.io/OS/`.

## 2. Move the domain to Cloudflare (one time)
Cloudflare Tunnel needs the domain on Cloudflare DNS:
1. Cloudflare dashboard -> Add a site -> enter your domain (Free plan).
2. Cloudflare gives you two nameservers; set them at Squarespace (Domains ->
   DNS -> Nameservers -> use custom). Propagation ~minutes-hours.

(Optional nice URL) add a DNS record `play -> CNAME -> olden-shire.github.io`
(DNS-only, grey cloud), then set `play.<domain>` as the Pages custom domain in
the repo Pages settings.

## 3. Cloudflare Tunnel on your PC (one time)
```powershell
winget install Cloudflare.cloudflared
cloudflared tunnel login                 # authorise your domain
cloudflared tunnel create os             # creates a tunnel + credentials json
cloudflared tunnel route dns os game.<domain>
```
Create `%USERPROFILE%\.cloudflared\config.yml`:
```yaml
tunnel: os
credentials-file: C:\Users\<you>\.cloudflared\<tunnel-id>.json
ingress:
  - hostname: game.<domain>
    service: http://localhost:40001   # cloudflared proxies the WS upgrade
  - service: http_status:404
```

## 4. Run it (each session)
```powershell
.\run-server.ps1            # or run-server-gui.ps1 - listens on 0.0.0.0:40001
cloudflared tunnel run os   # in another terminal
```

## 5. The URL you hand out
```
https://play.<domain>/?server=game.<domain>
```
(or `https://olden-shire.github.io/OS/?server=game.<domain>` without the custom
domain). The `?server=` override points the client at your tunnel; on an HTTPS
page it connects with `wss://` automatically (`ws_socket.rs`). Locally, opening
the bundle without `?server=` still targets `127.0.0.1` as before.

## Notes
- Quick test without a custom domain / DNS move: `cloudflared tunnel --url http://localhost:40001`
  prints a throwaway `https://<random>.trycloudflare.com` - use it as
  `?server=<random>.trycloudflare.com`. URL changes each run.
- The server must allow the connection through any local firewall on 40001
  (cloudflared connects to it on localhost, so usually fine).
