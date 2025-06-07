
```shell
curl -fsSL -o ddns.toml https://raw.githubusercontent.com/lvillis/ddns-rs/main/ddns.example.toml
docker run -d --name=ddns-rs \
  -p 8080:8080 \
  -v $PWD/ddns.toml:/opt/app/ddns.toml \
  -e DDNS_HTTP_JWT_SECRET="$(openssl rand -hex 32)" \
  docker.io/lvillis/ddns-rs:latest
```