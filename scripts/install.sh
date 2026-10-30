#!/bin/sh
# Installs Statup as a systemd service, or updates it when it is installed:
#
#   curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh
#
# Every question has its option, and --yes takes the default of the others.
# Run with --help for the list.

set -eu

REPOSITORY=https://github.com/karl-cta/statup
SETTINGS_DIR=/etc/statup
SETTINGS=$SETTINGS_DIR/statup.env
UNIT=/etc/systemd/system/statup.service
WRAPPER=/usr/local/bin/statup

usage() {
    cat <<'EOF'
Installs Statup as a systemd service. Run again, it updates Statup and keeps
its settings.

Usage: install.sh [option...]

  --prefix <dir>        Program folder (/opt/statup)
  --data-dir <dir>      Data folder (/var/lib/statup)
  --backup-dir <dir>    Backup folder (<data folder>/backups)
  --port <port>         Port (3000)
  --listen all|local    Every address, or this machine only, behind a
                        reverse proxy; an address works too (all)
  --user <name>         Account the service runs as (statup)
  --firewall            Open the port when ufw or firewalld is active (asked)
  --no-firewall         Leave the firewall as it is
  --version <x.y.z>     Version to install (the latest)
  --archive <file>      Install from an archive already downloaded
  --mirror <url>        Download from a copy of the release files, served as
                        <url>/v<version>/<file>; needs --version
  --yes                 Take the default of every question not answered, and
                        confirm --uninstall
  --uninstall           Remove Statup; keep its data, settings and account
  --help                Show this help
EOF
}

say() {
    printf '%s\n' "$*"
}

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

usage_error() {
    printf 'Error: %s\n' "$*" >&2
    exit 2
}

has() {
    command -v "$1" >/dev/null 2>&1
}

parse_options() {
    while [ "$#" -gt 0 ]; do
        case $1 in
            --prefix | --data-dir | --backup-dir | --port | --listen | --user | --version | --archive | --mirror)
                [ "$#" -ge 2 ] || usage_error "$1 needs a value"
                case $1 in
                    --prefix) prefix=$2 ;;
                    --data-dir) data_dir=$2 ;;
                    --backup-dir) backup_dir=$2 ;;
                    --port) port=$2 ;;
                    --listen) listen=$2 ;;
                    --user) user=$2 ;;
                    --version) version=${2#v} ;;
                    --archive) archive=$2 ;;
                    --mirror) mirror=${2%/} ;;
                esac
                shift
                ;;
            --firewall) open_firewall=yes ;;
            --no-firewall) open_firewall=no ;;
            --yes | -y) assume_yes=1 ;;
            --uninstall) uninstall=1 ;;
            --help | -h)
                usage
                exit 0
                ;;
            *) usage_error "unknown option $1 (see --help)" ;;
        esac
        shift
    done
}

require_terminal() {
    if ! (exec </dev/tty) 2>/dev/null; then
        usage_error "no terminal to answer the questions: pass --yes, with the options you need"
    fi
}

# ask <variable> <question> <default>: the option given, else the answer,
# else the default.
ask() {
    eval "ask_value=\${$1:-}"
    if [ -z "$ask_value" ]; then
        if [ "$assume_yes" = 1 ]; then
            ask_value=$3
        else
            printf '%s [%s]: ' "$2" "$3" >/dev/tty
            read -r ask_value </dev/tty || ask_value=
            [ -n "$ask_value" ] || ask_value=$3
        fi
    fi
    eval "$1=\$ask_value"
}

# confirm <question> <default y|n>: succeeds on yes. --yes takes the default.
confirm() {
    if [ "$assume_yes" = 1 ]; then
        answer=$2
    else
        require_terminal
        if [ "$2" = y ]; then hint='Y/n'; else hint='y/N'; fi
        printf '%s [%s]: ' "$1" "$hint" >/dev/tty
        read -r answer </dev/tty || answer=
        [ -n "$answer" ] || answer=$2
    fi
    case $answer in
        y | Y | yes | Yes) return 0 ;;
        *) return 1 ;;
    esac
}

check_folder() {
    case $1 in
        /*) ;;
        *) usage_error "$1: give a full path, starting with /" ;;
    esac
    case $1 in
        *[!A-Za-z0-9/._-]*) usage_error "$1: use letters, digits, dots, dashes and underscores only" ;;
    esac
    case $1 in
        / | /bin | /boot | /dev | /etc | /home | /opt | /proc | /root | /run | /sbin | /srv | /sys | /tmp | /usr | /usr/local | /var | /var/lib)
            usage_error "$1: choose a folder of its own, not a system folder"
            ;;
    esac
}

check_choices() {
    for folder in "$prefix" "$data_dir" "$backup_dir"; do
        check_folder "$folder"
    done
    case $port in
        '' | *[!0-9]*) usage_error "port $port: give a number" ;;
    esac
    if [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
        usage_error "port $port: give a number from 1 to 65535"
    fi
    case $listen in
        all) host=0.0.0.0 ;;
        local) host=127.0.0.1 ;;
        *[!0-9A-Fa-f.:]* | '') usage_error "listen $listen: give all, local or an address" ;;
        *) host=$listen ;;
    esac
    case $user in
        [a-z_]*) ;;
        *) usage_error "account $user: start with a lowercase letter" ;;
    esac
    case $user in
        *[!a-z0-9_-]*) usage_error "account $user: use lowercase letters, digits, dashes and underscores" ;;
    esac
}

download() {
    if has curl; then
        curl -fsSL --retry 3 -o "$2" "$1"
    elif has wget; then
        wget -q -O "$2" "$1"
    else
        fail "curl or wget is needed to download Statup"
    fi
}

# The version GitHub's release page points to as the latest, read from its
# redirect: the API answers an office or a runner behind one address with a
# limit of 60 requests an hour.
latest_version() {
    if has curl; then
        location=$(curl -fsSI -o /dev/null -w '%{redirect_url}' "$REPOSITORY/releases/latest") || location=
    else
        location=$(wget --max-redirect=0 -S -O /dev/null "$REPOSITORY/releases/latest" 2>&1 |
            sed -n 's/^ *Location: *//p' | tr -d '\r' | head -n 1)
    fi
    printf '%s\n' "${location##*/tag/v}"
}

architecture() {
    case $(uname -m) in
        x86_64 | amd64) printf 'x86_64\n' ;;
        aarch64 | arm64) printf 'aarch64\n' ;;
        *) fail "Statup has no archive for $(uname -m): use Docker or build it from source" ;;
    esac
}

# Leaves the archive of the version asked in $tmp, its checksum verified,
# and sets $archive_file and $version.
fetch_archive() {
    if [ -n "${archive:-}" ]; then
        [ -f "$archive" ] || fail "$archive: no such file"
        case $(basename "$archive") in
            statup-*-linux-"$(architecture)".tar.gz) ;;
            *) fail "$archive is not a Statup archive for this machine ($(uname -m))" ;;
        esac
        archive_file=$archive
        version=$(basename "$archive")
        version=${version#statup-}
        version=${version%-linux-*}
        return
    fi
    if [ -z "${version:-}" ]; then
        [ -z "${mirror:-}" ] || usage_error "--mirror needs --version"
        version=$(latest_version)
    fi
    case $version in
        '' | *[!0-9A-Za-z.-]*) fail "cannot tell the latest version of Statup: pass --version" ;;
    esac
    name="statup-$version-linux-$(architecture).tar.gz"
    from="${mirror:-$REPOSITORY/releases/download}/v$version"
    say "Downloading Statup $version"
    download "$from/$name" "$tmp/$name" || fail "cannot download $from/$name"
    download "$from/SHA256SUMS" "$tmp/SHA256SUMS" || fail "cannot download $from/SHA256SUMS"
    # A file missing from the list fails too: sha256sum finds no line to check.
    (cd "$tmp" && grep " $name\$" SHA256SUMS | sha256sum -c - >/dev/null 2>&1) ||
        fail "the checksum of $name does not match: download refused"
    archive_file=$tmp/$name
}

unpack_archive() {
    mkdir "$tmp/unpacked"
    tar --no-same-owner -xzf "$archive_file" -C "$tmp/unpacked" || fail "$archive_file is not a Statup archive"
    unpacked=
    for folder in "$tmp"/unpacked/statup-*; do
        [ -f "$folder/statup" ] && [ -d "$folder/static" ] && unpacked=$folder
    done
    [ -n "$unpacked" ] || fail "$archive_file is not a Statup archive"
}

# The binary and static/ are swapped in by a rename, so that a running
# Statup keeps its files until it restarts.
install_program() {
    mkdir -p "$prefix"
    cp "$unpacked/statup" "$prefix/statup.new"
    chmod 755 "$prefix/statup.new"
    mv -f "$prefix/statup.new" "$prefix/statup"
    rm -rf "$prefix/static.new" "$prefix/static.old"
    cp -R "$unpacked/static" "$prefix/static.new"
    [ ! -d "$prefix/static" ] || mv "$prefix/static" "$prefix/static.old"
    mv "$prefix/static.new" "$prefix/static"
    rm -rf "$prefix/static.old"
    cp "$unpacked/README.md" "$unpacked/LICENSE" "$unpacked/THIRD_PARTY_NOTICES.md" "$prefix/"
    printf '%s\n' "$version" >"$prefix/VERSION"
    chown -R root:root "$prefix/static"
    chown root:root "$prefix/statup" "$prefix/VERSION" "$prefix/README.md" "$prefix/LICENSE" "$prefix/THIRD_PARTY_NOTICES.md"
}

create_account() {
    if id "$user" >/dev/null 2>&1; then
        return
    fi
    for shell in /usr/sbin/nologin /sbin/nologin /bin/false; do
        [ -x "$shell" ] && break
    done
    useradd --system --user-group --home-dir "$data_dir" --no-create-home --shell "$shell" "$user"
    say "Created the account $user"
}

prepare_folders() {
    group=$(id -gn "$user")
    for folder in "$data_dir" "$data_dir/uploads" "$backup_dir"; do
        mkdir -p "$folder"
        chown "$user:$group" "$folder"
        chmod 750 "$folder"
    done
}

write_settings() {
    mkdir -p "$SETTINGS_DIR"
    cat >"$SETTINGS" <<EOF
# Statup settings, read when it starts. After a change: systemctl restart statup
# Every setting: https://statup.dev/docs/configuration/
HOST=$host
PORT=$port
DATABASE_URL=$data_dir/statup.db
UPLOAD_DIR=$data_dir/uploads
BACKUP_DIR=$backup_dir
EOF
    chown "root:$group" "$SETTINGS"
    chmod 640 "$SETTINGS"
}

# setting <KEY>: its value in the settings file, the last one when repeated.
setting() {
    sed -n "s/^$1=//p" "$SETTINGS" | tail -n 1
}

read_settings() {
    host=$(setting HOST)
    port=$(setting PORT)
    database=$(setting DATABASE_URL)
    database=${database#sqlite://}
    database=${database#sqlite:}
    data_dir=$(dirname "$database")
    backup_dir=$(setting BACKUP_DIR)
    [ -n "$backup_dir" ] || backup_dir=$data_dir/backups
    [ -n "$host" ] || host=0.0.0.0
    [ -n "$port" ] || port=3000
}

write_wrapper() {
    cat >"$WRAPPER" <<EOF
#!/bin/sh
# The commands of Statup, run as its account with its settings.
exec runuser -u $user -- $prefix/statup --env $SETTINGS "\$@"
EOF
    chmod 755 "$WRAPPER"
}

write_unit() {
    protect_home=true
    for folder in "$data_dir" "$backup_dir"; do
        case $folder in
            /home/* | /root/* | /run/user/*) protect_home=read-only ;;
        esac
    done
    cat >"$UNIT" <<EOF
[Unit]
Description=Statup status page
Documentation=https://statup.dev/docs/
After=network-online.target
Wants=network-online.target
RequiresMountsFor=$data_dir $backup_dir

[Service]
Type=simple
User=$user
Group=$group
WorkingDirectory=$prefix
ExecStart=$prefix/statup --env $SETTINGS
Restart=on-failure
RestartSec=5s
# Ports below 1024 need this, whatever port the settings name now: it is
# the only power of root the service keeps.
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=$protect_home
ReadWritePaths=$data_dir $backup_dir

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
}

active_firewall() {
    if has ufw && ufw status 2>/dev/null | grep -q '^Status: active'; then
        printf 'ufw\n'
    elif has firewall-cmd && firewall-cmd --state >/dev/null 2>&1; then
        printf 'firewalld\n'
    fi
}

open_port() {
    case $host in
        127.0.0.1 | ::1) return 0 ;;
    esac
    firewall=$(active_firewall)
    if [ -z "$firewall" ]; then
        [ "${open_firewall:-}" != yes ] || say "No active firewall: nothing to open"
        return 0
    fi
    case ${open_firewall:-} in
        no) return 0 ;;
        yes) ;;
        *) confirm "Open port $port in $firewall?" y || return 0 ;;
    esac
    case $firewall in
        ufw) ufw allow "$port/tcp" comment Statup >/dev/null ;;
        firewalld) firewall-cmd --quiet --permanent --add-port="$port/tcp" && firewall-cmd --quiet --reload ;;
    esac
    say "Opened port $port in $firewall"
}

probe() {
    if has curl; then
        curl -fsS -o /dev/null "$1" 2>/dev/null
    else
        wget -q -O /dev/null "$1" 2>/dev/null
    fi
}

# Waits up to 30 s for Statup to answer on the address it listens on.
wait_for_statup() {
    case $host in
        0.0.0.0) target=127.0.0.1 ;;
        ::) target='[::1]' ;;
        *:*) target="[$host]" ;;
        *) target=$host ;;
    esac
    tries=0
    while [ "$tries" -lt 30 ]; do
        probe "http://$target:$port/health" && return 0
        tries=$((tries + 1))
        sleep 1
    done
    fail "Statup does not answer on port $port: see journalctl -u statup -e"
}

address() {
    case $host in
        0.0.0.0 | ::)
            ip=$(hostname -I 2>/dev/null | cut -d' ' -f1) || ip=
            printf 'http://%s:%s\n' "${ip:-$(hostname)}" "$port"
            ;;
        *:*) printf 'http://[%s]:%s\n' "$host" "$port" ;;
        *) printf 'http://%s:%s\n' "$host" "$port" ;;
    esac
}

ask_choices() {
    [ "$assume_yes" = 1 ] || require_terminal
    ask prefix "Program folder" /opt/statup
    if [ -f "$SETTINGS" ]; then
        say "Keeping the settings of $SETTINGS"
        read_settings
        listen=$host
    else
        ask data_dir "Data folder" /var/lib/statup
        ask backup_dir "Backup folder" "$data_dir/backups"
        ask port "Port" 3000
        ask listen "Listen on every address (all), or on this machine only, behind a reverse proxy (local)" all
    fi
    ask user "Account of the service" statup
    check_choices
}

install() {
    ask_choices
    fetch_archive
    unpack_archive
    say "Installing Statup $version in $prefix"
    create_account
    prepare_folders
    install_program
    [ -f "$SETTINGS" ] || write_settings
    write_wrapper
    write_unit
    systemctl enable --now statup >/dev/null 2>&1 || fail "the service does not start: see journalctl -u statup -e"
    open_port
    wait_for_statup
    say ""
    say "Statup $version is running: $(address)"
    say "Open it to create the administrator account. Settings: $SETTINGS"
    say "Help: https://statup.dev/docs/"
}

installed_choices() {
    prefix=$(sed -n 's/^WorkingDirectory=//p' "$UNIT")
    user=$(sed -n 's/^User=//p' "$UNIT")
    if [ -z "$prefix" ] || [ -z "$user" ]; then
        fail "$UNIT was not written by this script"
    fi
}

refuse_downgrade() {
    installed=$(cat "$prefix/VERSION" 2>/dev/null) || installed=
    if [ -z "$installed" ] || [ "$installed" = "$version" ]; then
        return 0
    fi
    oldest=$(printf '%s\n%s\n' "$installed" "$version" | sort -V | head -n 1)
    [ "$oldest" = "$version" ] || return 0
    confirm "Statup $installed is installed: install the older $version? Its database may be too new for it." n ||
        fail "kept Statup $installed"
}

update() {
    if [ -n "${prefix:-}${data_dir:-}${backup_dir:-}${port:-}${listen:-}${user:-}" ]; then
        say "Statup is installed: the options of a new install are ignored, its settings are in $SETTINGS"
    fi
    installed_choices
    [ -f "$SETTINGS" ] || fail "$SETTINGS is missing: remove Statup with --uninstall, then install it again"
    read_settings
    say "Updating Statup in $prefix, with its settings"
    fetch_archive
    unpack_archive
    refuse_downgrade
    prepare_folders
    install_program
    write_wrapper
    write_unit
    systemctl restart statup
    wait_for_statup
    say "Statup $version is running: $(address)"
}

# Only the files the script installed leave the program folder.
uninstall() {
    if [ ! -f "$UNIT" ]; then
        say "Statup is not installed as a service here"
        return
    fi
    installed_choices
    kept="the account $user and any firewall rule"
    if [ -f "$SETTINGS" ]; then
        read_settings
        kept="the data in $data_dir and $backup_dir, the settings in $SETTINGS, $kept"
    fi
    if [ "$assume_yes" != 1 ]; then
        confirm "Remove Statup from $prefix? Its data, settings and account stay." n || fail "nothing removed"
    fi
    systemctl disable --now statup >/dev/null 2>&1 || true
    rm -f "$UNIT" "$WRAPPER"
    systemctl daemon-reload
    rm -rf "$prefix/static" "$prefix/static.new" "$prefix/static.old"
    rm -f "$prefix/statup" "$prefix/statup.new" "$prefix/VERSION" "$prefix/README.md" "$prefix/LICENSE" "$prefix/THIRD_PARTY_NOTICES.md"
    rmdir "$prefix" 2>/dev/null || true
    say "Statup is removed. Kept: $kept."
}

main() {
    assume_yes=0
    uninstall=0
    parse_options "$@"
    [ "$(id -u)" -eq 0 ] || fail "run it as root, for example with sudo"
    [ -d /run/systemd/system ] || fail "this script needs systemd. Elsewhere, use Docker: https://statup.dev/docs/install/"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    trap 'rm -rf "$tmp"; exit 1' INT TERM
    if [ "$uninstall" = 1 ]; then
        uninstall
    elif [ -f "$UNIT" ]; then
        update
    else
        install
    fi
}

# Last, so that a download cut short runs nothing.
main "$@"
