# fleet deployment

    sudo install -d /etc/trendkit
    printf 'GH_TOKEN=<pat>\nTRENDKIT_TELEGRAM_CHAT_ID=<chat>\n' | sudo tee /etc/trendkit/trendkit.env
    sudo chmod 600 /etc/trendkit/trendkit.env
    sudo cp deploy/trendkit-collect.{service,timer} /etc/systemd/system/
    sudo systemctl daemon-reload && sudo systemctl enable --now trendkit-collect.timer

`cargo build --release` once before enabling; the timer runs 08:00 with jitter.

## Note on mixed root/wyatt DB access

The timer runs as root (Telegram token file), manual runs as `wyatt`. After
the first root run, `trendkit.db-wal`/`-shm` may be root-owned; if a manual
`wyatt` run then fails to write, `sudo chown wyatt: /home/wyatt/dev/src/github.com/WyattAu/trendkit/trendkit.db*`
once fixes it. Or just let the timer own the DB and read reports via `--telegram`.
