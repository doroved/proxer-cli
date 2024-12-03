#!/bin/bash

# Check architecture
arch=$(uname -m)
if [[ "$arch" != "x86_64" && "$arch" != "arm64" ]]; then
    echo "Error: Unsupported architecture $arch. Exiting script."
    exit 1
fi

# Determine the appropriate architecture for the orb command
if [ "$arch" = "arm64" ]; then
    short_arch="aarch64"
else
    short_arch="x86_64"
fi

# Function to add PATH to the configuration file
update_path() {
    local rc_file=$1
    if ! grep -q "export PATH=.*proxer-cli/bin" "$rc_file"; then
        echo "# Proxer-cli" >> "$rc_file"
        echo "export PATH=\$PATH:~/.proxer-cli/bin" >> "$rc_file"
        source "$rc_file"
        echo "Updated $rc_file"
    else
        echo "Path already added in $rc_file"
    fi
}

# Fetch the latest release tag from GitHub
curl "https://api.github.com/repos/doroved/proxer-cli/releases/latest" |
    grep '"tag_name":' |
    sed -E 's/.*"([^"]+)".*/\1/' |
    xargs -I {} curl -OL "https://github.com/doroved/proxer-cli/releases/download/"\{\}"/proxer-cli.darwin-${short_arch}.tar.gz"

# Create directory for installation
mkdir -p ~/.proxer-cli/bin

# Extract and move the files
tar -xzvf ./proxer-cli.darwin-${short_arch}.tar.gz && \
    rm -rf ./proxer-cli.darwin-${short_arch}.tar.gz && \
    mv ./proxer-cli ~/.proxer-cli/bin

# Check for errors in the previous commands
if [ $? -ne 0 ]; then
    echo "Error. Exiting now."
    exit
fi

# Check if quarantine attribute exists before trying to remove it
if xattr ~/.proxer-cli/bin/proxer-cli | grep -q "com.apple.quarantine"; then
    xattr -d com.apple.quarantine ~/.proxer-cli/bin/proxer-cli
    echo "Removed quarantine attribute from ~/.proxer-cli/bin/proxer-cli"
fi

# Check if config.json5 exists, if not, create it
if [ ! -f ~/.proxer-cli/config.json5 ]; then
    cat <<EOL > ~/.proxer-cli/config.json5
[
  {
    "name": "Proxer Free [DE] proxerver",
    "enabled": true,
    "scheme": "HTTPS",
    "host": "proxerver.freemyip.com",
    "port": 443,
    "auth_credentials": {
      "username": "proxerver",
      "password": "onelove"
    },
    "filter": [
      {
        "name": "YouTube",
        "domains": ["*.youtube.com", "*.googlevideo.com", "*.ggpht.com", "*.ytimg.com", "youtu.be"]
      },
      {
        "name": "Discord",
        "domains": [
          "discord.com",
          "*.discord.com",
          "*.discordapp.com",
          "discord-attachments-*.storage.googleapis.com",
          "*.discordapp.net",
          "gateway.discord.gg"
        ]
      },
      {
        "name": "Test",
        "domains": ["api.ipify.org"]
      }
    ]
  }
]
EOL
    echo "Created config.json5 in ~/.proxer-cli"
else
    echo "config.json5 already exists in ~/.proxer-cli"
fi

# Add to PATH
export PATH=$PATH:~/.proxer-cli/bin

# Check for shell config files and update PATH
if [ -f ~/.bashrc ]; then
    update_path ~/.bashrc
elif [ -f ~/.zshrc ]; then
    update_path ~/.zshrc
elif [ -f ~/.bash_profile ]; then
    update_path ~/.bash_profile
fi

# Success message with version
proxer_version=$(proxer-cli -V)
echo "Successfully installed $proxer_version"

# Run the proxer help command
echo ""
proxer-cli --help
echo ""
echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!";
echo "Please copy and paste this command into the terminal and press Enter:"
echo "export PATH=\$PATH:~/.proxer-cli/bin"
