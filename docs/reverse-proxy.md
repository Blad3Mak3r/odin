# Serving the dashboard behind a reverse proxy

`odin serve` speaks plain HTTP and binds to `127.0.0.1:7331`. To reach it from
other machines, put a TLS-terminating reverse proxy on the same host, or use
an SSH tunnel. The README's *Authentication* section explains the login
model; this page only covers the proxy.

What the proxy must do:

- Terminate TLS and send `X-Forwarded-Proto: https`, so the session cookie is
  marked `Secure`.
- Keep the original `Host` header, or declare the public address in
  `/etc/odin/config.toml` (see below). State-changing requests are refused
  when the browser's `Origin` does not match.
- Pass the real client address in `X-Forwarded-For` so login limits and the
  security log see the visitor and not the proxy.
- Not buffer or time out the long-lived connections: the live event stream
  (`/api/events/sse`) and the log and job WebSockets.

## Odin configuration

```toml
# /etc/odin/config.toml
[server]
public_url = "https://odin.example.com"   # origin only, no path
# Only when the proxy is not on this host (loopback is always trusted):
# trusted_proxies = ["172.16.0.0/12"]
```

Restart with `sudo systemctl restart odin.service`. Keep Odin bound to
`127.0.0.1` when the proxy runs on the same machine.

## Caddy

Caddy obtains certificates, sets `X-Forwarded-*` headers and proxies
WebSockets and streaming responses without extra options:

```caddyfile
odin.example.com {
    reverse_proxy 127.0.0.1:7331 {
        flush_interval -1
    }
}
```

`flush_interval -1` sends the event stream to the browser as it is produced.

## nginx

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    listen 443 ssl;
    http2 on;
    server_name odin.example.com;

    ssl_certificate     /etc/letsencrypt/live/odin.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/odin.example.com/privkey.pem;

    # Save file and mod uploads can be large; match what you allow in Odin.
    client_max_body_size 512m;

    location / {
        proxy_pass http://127.0.0.1:7331;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSockets (live logs, jobs).
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;

        # Server-sent events and long log streams.
        proxy_buffering off;
        proxy_read_timeout 1h;
    }
}
```

## Cloudflare Tunnel

See *Behind a Cloudflare Tunnel* in the README: set `public_url` and
`client_ip_header = "CF-Connecting-IP"`, and keep Odin on loopback.

## Checking it works

- Sign in, then open Settings: the session list shows your real address, not
  the proxy's. If it shows `127.0.0.1`, the proxy's `X-Forwarded-For` is not
  being trusted; revisit `trusted_proxies`.
- The browser's cookie is `__Host-odin_session` with the `Secure` flag.
- Open an instance's Logs tab; lines keep arriving without the connection
  dropping after a minute.
