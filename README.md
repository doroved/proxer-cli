# Proxer

Proxy manager all network requests on macOS + spoofDPI all direct connections. Currently works only with IPv4 HTTP(S) proxies.

![proxer screenshot](screenshot.png)

## How to use

1. Rename `proxer.example.json5` to `proxer.json5` and edit it.

2. Run `cargo build --release` to build the binary.

```bash
cargo build --release
```

3. Run `./target/release/proxer` to start Proxer.

```bash
./target/release/proxer
```

4. Or run it in background process using `nohup`.

```bash
nohup ./target/release/proxer >/dev/null 2>&1 &
```

5. Run `kill $(pgrep proxer)` to stop Proxer.

```bash
kill $(pgrep proxer)
```
