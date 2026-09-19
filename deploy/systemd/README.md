# systemd 部署

二进制包中的目录结构可直接用于安装 MoleSignal：

```bash
sudo useradd --system --home-dir /var/lib/molesignal \
  --shell /usr/sbin/nologin molesignal
sudo install -Dm755 bin/molesignal /usr/local/bin/molesignal
sudo install -d -o root -g molesignal -m 0750 /etc/molesignal
sudo install -o root -g molesignal -m 0640 \
  conf/config.toml /etc/molesignal/config.toml
sudo install -Dm644 deploy/systemd/molesignal.service \
  /etc/systemd/system/molesignal.service
```

交付通道是运行时元数据。在 `/etc/molesignal/molesignal.env` 中配置当前通道：

```ini
RELEASE_CHANNEL=alpha
```

根据环境修改 `/etc/molesignal/config.toml` 中的 PostgreSQL、对象存储及密钥配置，
然后启动服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now molesignal
sudo systemctl status molesignal
```

`molesignal.service` 使用 `/var/lib/molesignal` 作为状态目录，并通过
`CAP_NET_BIND_SERVICE` 支持在启用 TLS/ACME 时监听 80 和 443 端口。
