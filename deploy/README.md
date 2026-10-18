# fleet deployment

    sudo install -d /etc/trendkit
    printf 'GH_TOKEN=<pat>\nTRENDKIT_TELEGRAM_CHAT_ID=<chat>\n' | sudo tee /etc/trendkit/trendkit.env
    sudo chmod 600 /etc/trendkit/trendkit.env
    sudo cp deploy/trendkit-collect.{service,timer} /etc/systemd/system/
    sudo systemctl daemon-reload && sudo systemctl enable --now trendkit-collect.timer

`cargo build --release` once before enabling; the timer runs 08:00 with jitter.
