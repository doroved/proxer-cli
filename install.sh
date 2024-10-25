#!/bin/bash

# Check architecture
arch=$(uname -m)
if [[ "$arch" != "x86_64" && "$arch" != "arm64" ]]; then
    echo "Error: Unsupported architecture $arch. Exiting script."
    exit 1
fi

# Determine the appropriate architecture for the orb command
if [ "$arch" = "aarch64" ]; then
    short_arch="arm64"
else
    short_arch="x86_64"
fi

# Function to add PATH to the configuration file
update_path() {
    local rc_file=$1
    if ! grep -q "export PATH=.*proxer/bin" "$rc_file"; then
        echo "# Proxer" >> "$rc_file"
        echo "export PATH=\$PATH:~/.proxer/bin" >> "$rc_file"
        source "$rc_file"
        echo "Updated $rc_file"
    else
        echo "Path already added in $rc_file"
    fi
}

# Fetch the latest release tag from GitHub
curl "https://api.github.com/repos/doroved/proxer/releases/latest" |
    grep '"tag_name":' |
    sed -E 's/.*"([^"]+)".*/\1/' |
    xargs -I {} curl -OL "https://github.com/doroved/proxer/releases/download/"\{\}"/proxer.darwin-${short_arch}.tar.gz"

# Create directory for installation
mkdir -p ~/.proxer/bin

# Extract and move the files
tar -xzvf ./proxer.darwin-${short_arch}.tar.gz && \
    rm -rf ./proxer.darwin-${short_arch}.tar.gz && \
    mv ./proxer ~/.proxer/bin

# Check for errors in the previous commands
if [ $? -ne 0 ]; then
    echo "Error. Exiting now."
    exit
fi

# Check if quarantine attribute exists before trying to remove it
if xattr ~/.proxer/bin/proxer | grep -q "com.apple.quarantine"; then
    xattr -d com.apple.quarantine ~/.proxer/bin/proxer
    echo "Removed quarantine attribute from ~/.proxer/bin/proxer"
fi

# Check if config.json5 exists, if not, create it
if [ ! -f ~/.proxer/config.json5 ]; then
    cat <<EOL > ~/.proxer/config.json5
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
        "domains": ["*.youtube.com", "*.googlevideo.com", "*.ggpht.com"]
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
    echo "Created config.json5 in ~/.proxer"
else
    echo "config.json5 already exists in ~/.proxer"
fi

# Add to PATH
export PATH=$PATH:~/.proxer/bin

# Check for shell config files and update PATH
if [ -f ~/.bashrc ]; then
    update_path ~/.bashrc
elif [ -f ~/.zshrc ]; then
    update_path ~/.zshrc
elif [ -f ~/.bash_profile ]; then
    update_path ~/.bash_profile
fi

# Success message with version
proxer_version=$(proxer -V)
echo "Successfully installed $proxer_version"

# Run the proxer help command
echo ""
proxer --help
echo ""
echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!";
echo "Please copy and paste this command into the terminal and press Enter:"
echo "export PATH=\$PATH:~/.proxer/bin"
