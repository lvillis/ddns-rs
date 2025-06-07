
```shell
docker run -d \
  --name=ddns-rs \
  -p 8080:8080 \
  -v $PWD/ddns.toml:/opt/app/ddns.toml \
  docker.io/lvillis/ddns-rs:latest
```