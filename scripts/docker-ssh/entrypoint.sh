#!/bin/sh
set -eu

: "${DSHELL_TEST_PASSWORD:?DSHELL_TEST_PASSWORD is required}"
user="${DSHELL_TEST_USER:-dshelltest}"

if ! id "$user" >/dev/null 2>&1; then
    useradd --create-home --shell /bin/bash "$user"
fi
printf '%s:%s\n' "$user" "$DSHELL_TEST_PASSWORD" | chpasswd

ssh-keygen -A
mkdir -p /etc/ssh/sshd_config.d
cat > /etc/ssh/sshd_config.d/99-dshell-test.conf <<'EOF'
PasswordAuthentication yes
KbdInteractiveAuthentication yes
UsePAM yes
PermitRootLogin no
EOF
sshd -t
exec /usr/sbin/sshd -D -e
