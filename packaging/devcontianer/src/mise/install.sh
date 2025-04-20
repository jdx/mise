#!/bin/sh
set -e

MISE_CLI_INSTALLER_GPG_KEY="0x7413A06D"

# Installer options
export MISE_INSTALL_PATH="/usr/local/bin/mise"

# Feature options
SHIMS=${SHIMS:-false}
echo "Options:"
echo "- SHIMS: ${SHIMS}"

if [ "$(id -u)" -ne 0 ]; then
    echo -e 'Script must be run as root. Use sudo, su, or add "USER root" to your Dockerfile before running this script.'
    exit 1
fi

export DEBIAN_FRONTEND=noninteractive

# Get the list of GPG key servers that are reachable
get_gpg_key_servers() {
    declare -A keyservers_curl_map=(
        ["hkp://keyserver.ubuntu.com"]="http://keyserver.ubuntu.com:11371"
        ["hkp://keyserver.ubuntu.com:80"]="http://keyserver.ubuntu.com"
        ["hkps://keys.openpgp.org"]="https://keys.openpgp.org"
        ["hkp://keyserver.pgp.com"]="http://keyserver.pgp.com:11371"
    )

    local curl_args=""
    local keyserver_reachable=false  # Flag to indicate if any keyserver is reachable

    if [ ! -z "${KEYSERVER_PROXY}" ]; then
        curl_args="--proxy ${KEYSERVER_PROXY}"
    fi

    for keyserver in "${!keyservers_curl_map[@]}"; do
        local keyserver_curl_url="${keyservers_curl_map[${keyserver}]}"
        if curl -s ${curl_args} --max-time 5 ${keyserver_curl_url} > /dev/null; then
            echo "keyserver ${keyserver}"
            keyserver_reachable=true
        else
            echo "(*) Keyserver ${keyserver} is not reachable." >&2
        fi
    done

    if ! $keyserver_reachable; then
        echo "(!) No keyserver is reachable." >&2
        exit 1
    fi
}

# Import the specified key in a variable name passed in as
receive_gpg_keys() {
    local keys=${!1}
    local keyring_args=""
    if [ ! -z "$2" ]; then
        keyring_args="--no-default-keyring --keyring $2"
    fi

    # Install curl
    if ! type curl > /dev/null 2>&1; then
        check_packages curl
    fi

    # Use a temporary location for gpg keys to avoid polluting image
    export GNUPGHOME="/tmp/tmp-gnupg"
    mkdir -p ${GNUPGHOME}
    chmod 700 ${GNUPGHOME}
    echo -e "disable-ipv6\n$(get_gpg_key_servers)" > ${GNUPGHOME}/dirmngr.conf
    # GPG key download sometimes fails for some reason and retrying fixes it.
    local retry_count=0
    local gpg_ok="false"
    set +e
    until [ "${gpg_ok}" = "true" ] || [ "${retry_count}" -eq "5" ];
    do
        echo "(*) Downloading GPG key..."
        ( echo "${keys}" | xargs -n 1 gpg -q ${keyring_args} --recv-keys) 2>&1 && gpg_ok="true"
        if [ "${gpg_ok}" != "true" ]; then
            echo "(*) Failed getting key, retrying in 10s..."
            (( retry_count++ ))
            sleep 10s
        fi
    done
    set -e
    if [ "${gpg_ok}" = "false" ]; then
        echo "(!) Failed to get gpg key."
        exit 1
    fi
}

# Checks if packages are installed and installs them if not
check_packages() {
    if ! dpkg -s "$@" > /dev/null 2>&1; then
        apt_get_update
        apt-get -y install --no-install-recommends "$@"
    fi
}

install_mise_activate() {
    local shell=$1
    local rc=$2

    if ! command -v $shell > /dev/null 2>&1; then
        echo "(*) $shell not found. Skipping mise activate for $shell."
        return
    fi
    if [ ! -f $rc ]; then
        echo "(*) $rc not found. Skipping mise activate for $shell."
        return
    fi

    echo "Activating mise for $shell..."

    echo >> $rc
    echo "# Activate mise" >> $rc
    echo "eval \"\$(mise activate $shell)\"" >> $rc
    if [ "$SHIMS" = "true" ]; then
        echo "eval \"\$(mise activate $shell --shims)\"" >> $rc
    fi
}

check_packages curl ca-certificates apt-transport-https dirmngr gnupg2

# Import the GPG key for the mise CLI
. /etc/os-release
receive_gpg_keys MISE_CLI_INSTALLER_GPG_KEY /usr/share/keyrings/mise-installer-keyring.gpg

# Run the mise CLI installer
echo "Installing mise CLI..."
curl -#flo https://mise.jdx.dev/install.sh.sig | gpg --decrypt | sh

chmod +x ${MISE_INSTALL_PATH}
chown ${_REMOTE_USER} ${MISE_INSTALL_PATH}

install_mise_activate bash /etc/bash.bashrc
install_mise_activate zsh /etc/zsh/zshrc

# Clean up
rm -rf "/tmp/tmp-gnupg"
rm -rf /var/lib/apt/lists/*
