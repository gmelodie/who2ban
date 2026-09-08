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
2. Give the tunnel a public hostname, say `draft.example.com`, and point it at the service `http://app:8731`.
3. Put the token in `.env` as `TUNNEL_TOKEN`, fill in `BASIC_USER` and `BASIC_PASS`, and run `make`.

Reaching the docker socket is a root-equivalent privilege: anyone who can talk to it can start a container that mounts the host filesystem and write to it. Being in the `docker` group is therefore not a smaller permission than root, it is the same one held permanently. To keep the account running this out of that group, ask for it a command at a time:

```sh
make up DOCKER="sudo docker"
```

The app itself never wants any of it. The container drops to an unprivileged user, no capabilities, a read-only filesystem and a memory limit, and writes only the one volume.

The app asks for the login itself, so it stays behind a password wherever it is reached from. `make serve` sets neither variable and therefore asks for nothing, which is what you want on localhost.

## How it works

The client reads the `replay.server.battlelobby` file, which contains the ten battletags of the players in the current lobby. Then, it queries the database for the opponent's most played and most successful heroes and shows it to the user.
