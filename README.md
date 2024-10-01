# Proxer

Network request proxy manager with host filtering on macOS + spoofDPI direct connections. Currently only works with IPv4 HTTP(S) proxies.

Currently works only with IPv4 HTTP(S) proxies.

![proxer screenshot](screenshot.png)

## How to use

1. Clone the repository.

```bash
git clone https://github.com/doroved/proxer.git
```

2. Rename `proxer.example.json5` to `proxer.json5` and edit it.

```bash
mv proxer.example.json5 proxer.json5
```

3. Run `cargo build --release` to build the binary.

```bash
cargo build --release
```

4. Run `./target/release/proxer` to start Proxer.

```bash
./target/release/proxer
```

5. Or run it in background process using `nohup`.

```bash
nohup ./target/release/proxer >/dev/null 2>&1 &
```

6. Run `kill $(pgrep proxer)` to stop Proxer.

```bash
kill $(pgrep proxer)
```
