FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d \
    && chmod +x /usr/sbin/policy-rc.d \
    && sed -i '/^Suites:/ s/ noble-backports//' /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get update -o Acquire::Retries=5 \
    && apt-get install -o Acquire::Retries=5 -y --no-install-recommends \
        htop \
        openssh-server \
        tmux \
        vim \
    && rm /usr/sbin/policy-rc.d

COPY entrypoint.sh /usr/local/bin/dshell-ssh-test
RUN chmod 0755 /usr/local/bin/dshell-ssh-test && mkdir -p /run/sshd

EXPOSE 22
ENTRYPOINT ["/usr/local/bin/dshell-ssh-test"]
