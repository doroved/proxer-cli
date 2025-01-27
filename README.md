# Proxer-cli

> **Subscribe to us on [Telegram](https://t.me/macproxer) to receive notifications about new versions and updates.**

> Check out the desktop version of [Proxer](https://github.com/doroved/proxer).

Network request proxy manager with host filtering on macOS.

![proxer-cli screenshot](screenshot.png)

## How to Install

Just log into your macOS terminal and run the command:

```bash
curl -fsSL https://raw.githubusercontent.com/doroved/proxer-cli/main/install.sh | bash
```

After installation, be sure to run this command to make proxer available in the current terminal session:

```bash
export PATH=$PATH:~/.proxer-cli/bin
```

To update proxer to the latest version, use the same command that was used for installation.

## Key Features:

- Traffic filtering by hosts.
- Support for HTTP(S) proxies with authentication via username/password or token (must match the token specified in the configuration file [proxerver-cli](https://github.com/doroved/proxerver-cli)).

```
proxer-cli --help

Proxy TCP traffic on macOS with domain filtering.

Usage: proxer-cli [OPTIONS]

Options:
      --port <u16>       Set port for proxer. By default 5555.
      --config <string>  Path to the configuration file. Example: '/path/to/config.json'. Default is ~/.proxer-cli/config.json.
  -h, --help             Print help
  -V, --version          Print version
```

The default configuration file is located in `~/.proxer-cli/config.json`. To edit it, you can quickly open it using the terminal command:

```bash
open -a TextEdit ~/.proxer-cli/config.json
```

If you want to use your own configuration file, you can specify it at startup using the `--config` flag.
For example, if you are in a directory with the config, you can run proxer as follows:

```bash
proxer-cli --config ./config.json
```

Configuration file structure:

```json
[
  {
    "name": "Proxerver Test",
    "enabled": true,
    "scheme": "HTTPS",
    "host": "proxerver.freemyip.com",
    "port": 443,
    "auth": {
      "credentials": {
        "username": "",
        "password": ""
      },
      "token": "fe3181d2f5e5e529efa280a474c0e949997c604518624893b1cd0e7c4c8f3727"
    },
    "rules": [
      {
        "name": "YouTube",
        "hosts": [
          "youtu.be",
          "*.googlevideo.com",
          "*.youtube.com",
          "*.ytimg.com",
          "*.ggpht.com",
          "*.googleapis.com"
        ]
      },
      {
        "name": "Discord",
        "hosts": ["*discord*.*"]
      },
      {
        "name": "Test",
        "hosts": ["api.ipify.org"]
      }
    ]
  }
]
```

## Configuration File Overview

The configuration file is a JSON array containing proxy settings. Each proxy configuration includes the following fields:

- **name**: A descriptive name for the proxy (e.g., "Proxerver Test").
- **enabled**: A boolean value indicating whether the proxy is active (`true` or `false`).
- **scheme**: The protocol used by the proxy ("HTTP or HTTPS").
- **host**: The hostname or IP address of the proxy server.
- **port**: The port number on which the proxy server listens (e.g., 443).
- **auth**: An object containing authentication details:
  - **credentials**: An object with `username` and `password` fields for basic authentication.
  - **token**: A token for authentication, if required.
- **rules**: An array of objects defining traffic rules:
  - **name**: A descriptive name for the filter (e.g., "YouTube").
  - **hosts**: An array of domain patterns to be filtered (e.g., `["*.youtube.com"]`).

If your proxies don't require authentication, you can leave the `auth.credentials.username` and `auth.credentials.password` or `auth.token` fields empty.

## Examples of Proxer Launch Commands

Set port 6666 for the local Proxer server.

```bash
proxer-cli --port 6666
```

To run the Proxer in the background, use nohup, for example:

```bash
nohup proxer-cli [OPTIONS] >/dev/null 2>&1 &
```

Running the Proxer in the background using nohup and saving the output to a file:

```bash
nohup proxer-cli [OPTIONS] > ~/.proxer-cli/log.txt 2>&1 &
```

## Local Build and Run

1. Clone the repository.

```bash
git clone https://github.com/doroved/proxer-cli.git
```

2. Run `cargo build --release` to build the binary.

```bash
cargo build --release
```

3. Run the Proxer binary with configuration.

```bash
./target/release/proxer-cli --config 'config.dev.json'
```

4. Or run it in background process using `nohup`.

```bash
nohup ./target/release/proxer-cli --config 'config.dev.json' >/dev/null 2>&1 &
```

5. To stop Proxer, run this command.

```bash
kill $(pgrep proxer-cli)
```

6. See Proxer running on your machine

```bash
lsof -i -P | grep LISTEN | grep proxer-cli
```

## Interesting projects

- [DumbProxy](https://github.com/SenseUnit/dumbproxy) - Great proxy server with various features
- [SpoofDPI](https://github.com/xvzc/SpoofDPI) - macOS
- [GoodbyeDPI](https://github.com/ValdikSS/GoodbyeDPI) - Windows
- [ByeDPI](https://github.com/hufrea/byedpi) - Windows, Linux
