# who2ban

See the most popular heroes among your opponents!

## Run it

```sh
make app      # the desktop app from source
make serve    # the admin console on http://localhost:8731
make          # the console on this machine, published over a Cloudflare tunnel
make test     # the whole workspace
```

## Publishing it

The console runs on a machine of your own, and Cloudflare gives it a name. Nothing listens on a public port, so the machine needs no public address and no certificate.

1. In the Cloudflare dashboard, under Zero Trust > Networks > Tunnels, create a tunnel and copy its token.
2. Give the tunnel a public hostname, say `draft.example.com`, and point it at the service `http://app:8731`. That is the compose service name. `localhost` there is the cloudflared container, which listens on nothing.
3. Check that the name resolves: `dig +short draft.example.com` must answer. The dashboard writes the DNS record for you only when the tunnel and the domain sit in one Cloudflare account. Otherwise add it yourself, as a **proxied** CNAME from `draft` to `<tunnel-id>.cfargotunnel.com`. An unproxied one does not resolve.
4. Put the token in `.env` as `TUNNEL_TOKEN`, fill in `BASIC_USER` and `BASIC_PASS`, and run `make`.

`curl -si https://draft.example.com/` then answers 401. That is the console asking for the login, and it means every layer works.

The app asks for the login itself, so it stays behind a password wherever it is reached from. `make serve` sets neither variable and therefore asks for nothing, which is what you want on localhost.

## How it works

The client reads the `replay.server.battlelobby` file, which contains the ten battletags of the players in the current lobby. Then, it queries the database for the opponent's most played and most successful heroes and shows it to the user.
