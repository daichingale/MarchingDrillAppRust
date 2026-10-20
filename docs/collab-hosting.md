# Hosting the relay and the phone page

The phone page is static files. The relay is one small process that stays
awake for the whole rehearsal. A sleeping free host drops the room, so the
relay wants the cheapest machine that does not sleep. The page can sit on a
free static host.

Both the phone and the PC speak `wss://` to that public name. On a school
LAN the same binaries still accept `ws://` and the bill is zero.

## What you publish

Phone page, from a machine with the Rust wasm target. The `wasm-bindgen`
CLI has to be the same version as the `wasm-bindgen` crate in
`Cargo.lock` (0.2.126 in this tree):

```bash
cargo build --release --target wasm32-unknown-unknown -p drill-web
wasm-bindgen --target web --out-dir crates/drill-web/web/pkg \
  target/wasm32-unknown-unknown/release/drill_web.wasm
```

Upload the whole `crates/drill-web/web` directory (`index.html` plus `pkg/`).
The page is plain files. It does not need a server process.

Relay binary:

```bash
cargo build --release -p drill-relay
# target/release/drill-relay
```

Listeners:

- `drill-relay 0.0.0.0:8787` is plain `ws://`. Use this behind a host that
  terminates TLS, and on a LAN.
- `drill-relay 0.0.0.0:8787 --tls cert.pem key.pem` is `wss://` in the
  process itself. The same pair can come from `DRILLFORGE_TLS_CERT` and
  `DRILLFORGE_TLS_KEY`.

Clients check the certificate. A public certificate from the host's proxy
is trusted automatically. A private certificate needs
`DRILLFORGE_TLS_CA=/path/ca.pem` on the PC. The phone trusts whatever the
browser trusts.

In the phone's host box, and in the PC's relay host box, type the same
value, for example `wss://drill-relay.fly.dev`. Leave the port off for
443. Share a long room code. Twenty people can drag. The next person
watches, and takes a seat when an editor leaves.

## Relay: Fly.io, about $2.19 per month

Fly.io no longer has a free allowance for new accounts. The smallest
always-on machine, `shared-cpu-1x` with 256 MB in Ashburn, is **$2.19 per
30 days**, billed by the second ([Fly.io pricing](https://docs.fly.io/about/pricing)).
The relay's measured footprint at 20 editors was about 45 MB, so 256 MB
is enough. The first ten single-hostname certificates are free. A shared
IPv4 is included. Outbound traffic is $0.02/GB in North America and
Europe; a rehearsal does not move much.

Fly's edge terminates TLS and forwards WebSocket to the process. The
binary listens with plain `ws://` on the internal port. Phones and PCs
still connect with `wss://`.

`Dockerfile` at the repo root of the deploy:

```dockerfile
FROM rust:1.99 AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p drill-relay

FROM debian:bookworm-slim
RUN useradd --create-home drill
USER drill
COPY --from=build /src/target/release/drill-relay /usr/local/bin/drill-relay
EXPOSE 8787
CMD ["drill-relay", "0.0.0.0:8787"]
```

`fly.toml`:

```toml
app = "drill-relay"
primary_region = "iad"

[http_service]
  internal_port = 8787
  force_https = true
  auto_stop_machines = "off"
  auto_start_machines = true
  min_machines_running = 1

[[vm]]
  size = "shared-cpu-1x"
  memory = "256mb"
```

```bash
fly launch --no-deploy
fly deploy
```

`auto_stop_machines = "off"` and `min_machines_running = 1` are what keep
the room from vanishing between songs. Stopping the machine to save the
$2.19 also stops the rehearsal.

## Relay: Render's free web service sleeps

Render's free web service is **$0** and includes about 750 instance hours
a month, but it **spins down after 15 minutes** without a request and
takes about a minute to wake. A WebSocket room does not survive that.
Use it only to try the binary. For a real rehearsal, keep the process
awake (Fly.io above, or a Render instance you pay to leave running).

If you do try the free service: new Web Service, Docker or native Rust,
start command `drill-relay 0.0.0.0:$PORT`, health check off (the relay
speaks WebSocket, not a browser page). The public URL is already `wss://`.

## Phone page: GitHub Pages, $0

For a public repository, GitHub Pages is free: about 1 GB of files and a
soft 100 GB of bandwidth a month, with HTTPS.

1. Put `index.html` and `pkg/` in a branch or a `docs/` folder.
2. Settings → Pages → deploy that folder.
3. Open `https://<user>.github.io/<repo>/`.
4. In the page, set the host to `wss://drill-relay.fly.dev` and a room
   code, then Join.

The page is `https`, so a bare host name would also use `wss://`. Typing
`wss://` is explicit and works even if the page is opened from a file.

## Phone page: Vercel Hobby, $0 for non-commercial use

Vercel Hobby hosts the same static folder, with a custom domain and TLS,
at no charge for personal non-commercial projects. Import the directory
as a static project (no framework). A site that charges money is outside
that plan.

Drag the `web` folder onto a new Vercel project, or:

```bash
npx vercel crates/drill-web/web --prod
```

## School LAN, $0

Skip the hosts. On one PC:

```bash
drill-relay 0.0.0.0:8787
```

Everyone types `ws://<that-pc>:8787` and the same room code. Phones have
to be on the same network. This is the path with no certificate and no bill.

## Cost

| Piece | Host | Cost | Stays awake |
| --- | --- | --- | --- |
| Relay | Fly.io shared-cpu-1x, 256 MB | about $2.19 / 30 days | yes |
| Relay | Render free web service | $0 | no, sleeps after 15 minutes |
| Relay | school LAN `ws://` | $0 | while that PC is on |
| Phone page | GitHub Pages | $0 | static |
| Phone page | Vercel Hobby (non-commercial) | $0 | static |

The practical pair is **Fly.io for the relay and GitHub Pages for the
page**: about **$2.19 per month**, one public `wss://` name, and the
phones never need the rehearsal Wi-Fi.
