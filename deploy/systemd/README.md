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
sudo install -Dm644 mmdb/dbip-city-lite.mmdb \
  /usr/share/molesignal/mmdb/dbip-city-lite.mmdb
```

`mmdb/dbip-city-lite.mmdb` 是 DB-IP City Lite 库（CC BY 4.0），打包时下载的当月版本，`[mmdb] db_path`
默认指向上面的安装路径。RUM 写入用它按客户端 IP 查国家，服务不会在运行期下载或更新它，升级时随新版本的
二进制一起替换。IP 地理位置数据由 DB-IP（https://db-ip.com）提供，署名与许可见二进制包里的
`mmdb/NOTICE.txt`。旧版本安装的 `/usr/share/molesignal/mmdb/GeoLite2-City.mmdb` 可以删除；如果
`/etc/molesignal/config.toml` 里的 `db_path` 仍指向它，改成上面的路径。

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
