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
TrustedUserCAKeys /etc/ssh/dshell_user_ca.pub
EOF
while [ ! -s /etc/ssh/dshell_user_ca.pub ]; do
    sleep 0.1
done
sshd -t
nohup socat TCP-LISTEN:3389,bind=127.0.0.1,reuseaddr,fork EXEC:/bin/cat >/dev/null 2>&1 &
nohup socat UNIX-LISTEN:/run/dshell-forward.sock,fork,unlink-early,mode=777 EXEC:/bin/cat >/dev/null 2>&1 &
exec /usr/sbin/sshd -D -e
