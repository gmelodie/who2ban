# who2ban

See the most popular heroes among your opponents!

## Run it

```sh
make app      # the desktop app from source
make serve    # the admin console on http://localhost:8731
make          # the console behind nginx and a certificate
make test     # the whole workspace
```

## How it works

The client reads the `replay.server.battlelobby` file, which contains the ten battletags of the players in the current lobby. Then, it queries the database for the opponent's most played and most successful heroes and shows it to the user.
