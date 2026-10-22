#!/bin/sh
set -eu

: "${DSHELL_TEST_PASSWORD:?DSHELL_TEST_PASSWORD is required}"
user="${DSHELL_TEST_USER:-dshelltest}"

if ! id "$user" >/dev/null 2>&1; then
    useradd --create-home --shell /bin/bash "$user"
fi
printf '%s:%s\n' "$user" "$DSHELL_TEST_PASSWORD" | chpasswd

ssh-keygen -A
printf '\nPasswordAuthentication yes\nPermitRootLogin no\n' >> /etc/ssh/sshd_config
sshd -t
exec /usr/sbin/sshd -D -e
